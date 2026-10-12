//! Tree-walking interpreter over the typed AST.
//!
//! This is the codegen-free execution path: it runs a parsed [`Program`]
//! directly, with no dependency on `inkwell`/LLVM. It exists so `axon run`
//! works end-to-end (and the Phase-10 `goal.md → goal.ax → result` story is
//! runnable) while the native codegen build is slow/blocked.
//!
//! For an LLM-orchestration workload the hot path is network latency, not
//! arithmetic, so a tree-walker is the appropriate execution model — native
//! codegen is a later performance concern, not a prerequisite.
//!
//! ## Scope (M1)
//! Implements the full deterministic core: scalars, arithmetic, `if`/`match`,
//! `let`/assignment, `while`/`while let`/`for`, user functions + recursion,
//! closures (lambdas with capture), structs, enum ADTs, `Option`/`Result`,
//! arrays, `?` propagation, string interpolation, impl-method dispatch, and a
//! broad builtin set (I/O, conversion, math, `str_*`, `assert*`).
//!
//! The ASI builtins (`ai_complete`, `ai_extract_*`, `goal_run`, the
//! `uncertain_*`/`temporal_*` family) are stubbed with a clear runtime error —
//! wiring them to `axon-ai`/`axon-rt` is M2.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io::Write as _;
use std::rc::Rc;

use crate::ast::{
    BinOp, EnumDef, Expr, FnDef, ImplBlock, Item, Literal, Pattern, Program, Stmt, TypeDef, UnaryOp,
};

// ── Runtime values ────────────────────────────────────────────────────────────

/// A runtime value produced by evaluating an expression.
///
/// AX-46: every payload is at most one pointer wide, so a `Value` is 16 bytes
/// (tag + one word) and `R` is 24 — pinned by `VALUE_SIZE_IS_PINNED` below.
/// Records, closures, handles and decimals live behind an `Rc`: cloning one
/// bumps a refcount, and the rare in-place write (`p.x = v`) goes through
/// [`Rc::make_mut`], which copies only when another binding still holds the
/// record — the same value semantics `Array` and `Str` already have.
#[derive(Debug, Clone)]
pub enum Value {
    /// Default i64 integer (I-7: integers default to `i64`).
    Int(i64),
    /// R19 Slice B — a width-aware integer value for non-i64 fixed-width types.
    /// Stores the value as i64 internally but carries its declared width so ops
    /// can apply width-correct masks (u8=0xFF, u16=0xFFFF, …). Only non-i64
    /// integer widths use this variant; `Int(i64)` remains the default, keeping
    /// the ~102 builtin `Int` sites untouched (blast-radius isolation, spec §11).
    SizedInt {
        val: i64,
        ty: IntWidth,
    },
    Float(f64),
    /// R21 — exact fixed-point decimal: i128 mantissa at `decimal::SCALE` (9 dp).
    /// Money-safe: exact arithmetic, no binary floating error. Behind an `Rc`
    /// because an inline `i128` would make every `Value` 32 bytes (16-aligned).
    Decimal(Rc<i128>),
    Bool(bool),
    /// String value. Same VALUE-semantics-over-shared-storage contract as
    /// `Array`: cloning a `Value` (env lookup, argument passing, `len(s)`) bumps
    /// a refcount instead of copying the bytes, and the only in-place write —
    /// `s + t` appending to an operand nobody else holds — goes through
    /// [`Rc::make_mut`]. `Rc<String>` rather than `Rc<str>` so that append can
    /// grow the buffer in place (amortized O(len t)) instead of reallocating
    /// (AX-31: a plain `String` made every string read O(len)).
    Str(Rc<String>),
    Unit,
    /// Array value. VALUE semantics, shared representation: cloning a `Value`
    /// (env lookup, argument passing, a struct field read) bumps a refcount
    /// instead of deep-copying the elements, and every in-place write goes
    /// through [`Rc::make_mut`], which copies only when the vector is shared.
    /// So `let b = a; b[0] = 9` still leaves `a` untouched, while `a[i] = v` on
    /// a uniquely owned binding is O(1) (AX-06: the deep copy made index reads
    /// and writes O(len), turning a sieve quadratic).
    Array(Rc<Elems>),
    /// Structural record: `Point { x, y }`.
    Struct(Rc<StructVal>),
    /// Enum variant: `Shape::Circle { radius }`.
    Enum(Rc<EnumVal>),
    Some(VBox),
    None,
    Ok(VBox),
    Err(VBox),
    /// A lambda plus the environment it captured at creation time.
    ///
    /// AUDIT T40 (findings F094 / P5-16 / DOC-02). The capture used to be a
    /// plain `HashMap<String, Value>` — a fresh clone per call — so an
    /// assignment to a captured binding inside the lambda was silently DROPPED
    /// when the call returned. Native codegen heap-allocates the capture and
    /// the write persists, so the same source printed different answers on
    /// the two engines with no error from either:
    ///
    ///   let n = 0; let bump = || { n = n + 1  n }
    ///   interp:  call1=1 call2=1 call3=1   outer n=0
    ///   native:  call1=1 call2=2 call3=3   outer n=0
    ///
    /// The shared `Rc` makes the capture cell PERSISTENT ACROSS CALLS of this
    /// closure (matching codegen) while still being a by-value snapshot of the
    /// defining scope — note `outer n=0` on both engines: the outer binding is
    /// not aliased. Cloning a closure value shares the same cell, which is what
    /// makes `let b = bump` observe the same counter.
    Closure(Rc<ClosureVal>),
    /// A channel — a shared FIFO queue. Cloning shares the same channel (Rc), so
    /// a `spawn`ed body and the main flow see the same queue. The interpreter is
    /// cooperative/single-threaded: `spawn` runs eagerly, so a `send` happens
    /// before the matching `recv`.
    Chan(Rc<RefCell<Queue>>),
    /// Tuple value `(a, b, …)`. Accessed via `t.0`, `t.1` (numeric field).
    /// Immutable once built, so the elements are shared on clone.
    Tuple(Rc<Elems>),
    /// String-keyed dictionary — the ASI workhorse for caches, frequency
    /// tables, named state. Mutating builtins (`dict_set`, `dict_remove`)
    /// share the inner `RefCell` so a stored handle stays in sync with
    /// the live state, matching the channel model. Keys are `str` only
    /// (not arbitrary `Value`) — covers 95% of ASI use cases without
    /// requiring `Hash + Eq` on the full Value enum.
    Dict(Rc<RefCell<DictMap>>),
    /// R13 native FFI: an opaque, affine native handle — see [`HandleVal`].
    Handle(Rc<HandleVal>),
}

/// AX-46: the sizes the evaluator's hot path moves on every operation. A
/// payload wider than one word (an inline `String`, `HashMap`, `i128`, or a
/// second pointer) grows every `Value` and every `R`; this fails the build
/// instead of silently taxing every operation again.
const VALUE_SIZE_IS_PINNED: () = {
    assert!(std::mem::size_of::<Value>() == 16);
    assert!(std::mem::size_of::<Flow>() == 24);
    assert!(std::mem::size_of::<R>() == 24);
};
const _: () = VALUE_SIZE_IS_PINNED;

/// The width of a [`Value::SizedInt`]: a fixed-width integer type other than
/// `i64` (which is plain [`Value::Int`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IntWidth {
    I8,
    I16,
    I32,
    U8,
    U16,
    U32,
    U64,
}

impl IntWidth {
    /// The width of semantic type `ty`, if it is a non-i64 integer type.
    pub fn of_type(ty: &crate::types::Type) -> Option<IntWidth> {
        use crate::types::Type;
        Some(match ty {
            Type::I8 => IntWidth::I8,
            Type::I16 => IntWidth::I16,
            Type::I32 => IntWidth::I32,
            Type::U8 => IntWidth::U8,
            Type::U16 => IntWidth::U16,
            Type::U32 => IntWidth::U32,
            Type::U64 => IntWidth::U64,
            _ => return None,
        })
    }

    /// The semantic type this width denotes.
    pub fn to_type(self) -> crate::types::Type {
        use crate::types::Type;
        match self {
            IntWidth::I8 => Type::I8,
            IntWidth::I16 => Type::I16,
            IntWidth::I32 => Type::I32,
            IntWidth::U8 => Type::U8,
            IntWidth::U16 => Type::U16,
            IntWidth::U32 => Type::U32,
            IntWidth::U64 => Type::U64,
        }
    }

    /// The source spelling: `u8`, `i32`, …
    pub fn name(self) -> &'static str {
        match self {
            IntWidth::I8 => "i8",
            IntWidth::I16 => "i16",
            IntWidth::I32 => "i32",
            IntWidth::U8 => "u8",
            IntWidth::U16 => "u16",
            IntWidth::U32 => "u32",
            IntWidth::U64 => "u64",
        }
    }
}

/// The fields of a record (struct or enum-variant) value. AX-47: names are
/// interned [`Sym`]s, and a record built from a literal of a declared type
/// stores its fields in the definition's order, so two values of one type
/// have the same layout. A field assigned that the type does not declare
/// (`r.extra = v`, structural typing) is appended.
///
/// Every renderer orders fields by NAME, as it did when this was a `HashMap`,
/// so the storage order is never observable.
#[derive(Clone, Default)]
pub struct Fields(Slot<Vec<(Sym, Value)>>);

impl Fields {
    /// Fields from `(name, value)` pairs; a repeated name keeps the last value.
    pub(crate) fn from_pairs(pairs: impl IntoIterator<Item = (Sym, Value)>) -> Fields {
        let mut f = Fields::default();
        for (k, v) in pairs {
            f.insert(k, v);
        }
        f
    }

    #[inline]
    pub(crate) fn get(&self, name: Sym) -> Option<&Value> {
        self.0.iter().find(|(k, _)| *k == name).map(|(_, v)| v)
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, name: Sym) -> Option<&mut Value> {
        self.0.iter_mut().find(|(k, _)| *k == name).map(|(_, v)| v)
    }

    /// The field spelled `name`. No field can be named something never
    /// interned, so this does not intern `name`.
    pub(crate) fn named(&self, name: &str) -> Option<&Value> {
        lookup_sym(name).and_then(|s| self.get(s))
    }

    /// Set field `name`, appending it if absent.
    pub(crate) fn insert(&mut self, name: Sym, v: Value) {
        match self.get_mut(name) {
            Some(slot) => *slot = v,
            None => self.0.push((name, v)),
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.0.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub(crate) fn iter(&self) -> impl Iterator<Item = (Sym, &Value)> {
        self.0.iter().map(|(k, v)| (*k, v))
    }

    /// The fields in place (a cast converts or wraps a field's value).
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = (Sym, &mut Value)> {
        self.0.iter_mut().map(|(k, v)| (*k, v))
    }

    /// The fields with their names, sorted by name: the order every
    /// renderer (display, literal form, shape, `SendValue`) uses.
    pub(crate) fn by_name(&self) -> Vec<(Rc<str>, &Value)> {
        let mut out: Vec<(Rc<str>, &Value)> =
            self.0.iter().map(|(k, v)| (sym_name(*k), v)).collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Same names with pairwise-equal values, in any order.
    pub(crate) fn equal(&self, other: &Fields) -> bool {
        if self.0.len() != other.0.len() {
            return false;
        }
        // Same type, same layout: one pass, no lookups.
        if self.0.iter().zip(other.0.iter()).all(|(a, b)| a.0 == b.0) {
            return self
                .0
                .iter()
                .zip(other.0.iter())
                .all(|(a, b)| value::values_equal(&a.1, &b.1));
        }
        self.0
            .iter()
            .all(|(k, v)| other.get(*k).is_some_and(|w| value::values_equal(v, w)))
    }
}

impl std::fmt::Debug for Fields {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_map()
            .entries(self.0.iter().map(|(k, v)| (sym_name(*k), v)))
            .finish()
    }
}

/// AX-59: where a container keeps the values it holds. On wasm32 the
/// container's `Drop` (`bounded_drop`) moves them out and drops them through
/// `drop_bounded`, so its own drop glue must not drop them again: the slot is
/// a `ManuallyDrop`. Native keeps the plain type and plain drop glue. Every
/// container keeps its slot private and derefs to its contents, so the same
/// code reads it on every target.
#[cfg(target_arch = "wasm32")]
type Slot<T> = std::mem::ManuallyDrop<T>;
#[cfg(not(target_arch = "wasm32"))]
type Slot<T> = T;

/// `x` in a [`Slot`].
#[cfg(target_arch = "wasm32")]
#[inline(always)]
fn slot<T>(x: T) -> Slot<T> {
    std::mem::ManuallyDrop::new(x)
}

/// `x` in a [`Slot`].
#[cfg(not(target_arch = "wasm32"))]
#[inline(always)]
fn slot<T>(x: T) -> Slot<T> {
    x
}

/// `Deref`/`DerefMut` of a newtype over a [`Slot`] to its contents.
macro_rules! slot_deref {
    ($t:ty => $target:ty) => {
        impl std::ops::Deref for $t {
            type Target = $target;
            #[inline]
            fn deref(&self) -> &$target {
                &self.0
            }
        }

        impl std::ops::DerefMut for $t {
            #[inline]
            fn deref_mut(&mut self) -> &mut $target {
                &mut self.0
            }
        }
    };
}

/// A dict's map. A newtype only so that, on wasm32, it can carry the AX-59
/// bounded `Drop`; it derefs to the map.
#[derive(Clone, Default)]
pub struct DictMap(Slot<std::collections::BTreeMap<String, Value>>);

slot_deref!(DictMap => std::collections::BTreeMap<String, Value>);

impl std::fmt::Debug for DictMap {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DictMap").field(&**self).finish()
    }
}

impl From<std::collections::BTreeMap<String, Value>> for DictMap {
    #[inline]
    fn from(m: std::collections::BTreeMap<String, Value>) -> DictMap {
        DictMap(slot(m))
    }
}

/// The elements of an array or a tuple (R50 S13). The representation is
/// private: `Vals` (a `Vec<Value>` in a [`Slot`], with the AX-59 bounded
/// `Drop` on wasm32) or a packed form (`Bools`, `Ints`, `Floats`) whose
/// elements stand for the plain `Value::Bool`, `Value::Int` and
/// `Value::Float` with those bits (a `SizedInt` is never packed). Only
/// `arr_repeat` of a plain scalar and `arr_range` build packed arrays
/// ([`Elems::repeat`], [`Elems::range`]); everything else is `Vals`. Every
/// reader sees the element `Value`s, so which arrays are packed is not
/// observable. A write or push of a plain scalar of the packed kind stays
/// packed; any other write, and any caller that needs the `Vec<Value>`
/// ([`Elems::vals_mut`], [`Elems::into_vec`]), converts the array to `Vals`
/// in place, once and for good. [`Elems::vals`] lends a read-only
/// `[Value]` (a temporary copy for a packed array; never on an element
/// read or write path).
#[derive(Clone)]
pub struct Elems(Rep);

/// [`Elems`]' representation.
#[derive(Clone)]
enum Rep {
    Vals(Slot<Vec<Value>>),
    Bools(Vec<bool>),
    Ints(Vec<i64>),
    Floats(Vec<f64>),
}

/// A borrowed view of an [`Elems`]' storage, for the element read paths
/// that work on packed storage in place.
pub(crate) enum ElemsRef<'a> {
    Vals(&'a [Value]),
    Bools(&'a [bool]),
    Ints(&'a [i64]),
    Floats(&'a [f64]),
}

/// A mutable view of an [`Elems`]' storage (element writes in place; the
/// length is fixed).
pub(crate) enum ElemsMut<'a> {
    Vals(&'a mut [Value]),
    Bools(&'a mut [bool]),
    Ints(&'a mut [i64]),
    Floats(&'a mut [f64]),
}

impl Default for Elems {
    #[inline]
    fn default() -> Elems {
        Elems(Rep::Vals(slot(Vec::new())))
    }
}

impl std::fmt::Debug for Elems {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

impl From<Vec<Value>> for Elems {
    #[inline]
    fn from(v: Vec<Value>) -> Elems {
        Elems(Rep::Vals(slot(v)))
    }
}

impl FromIterator<Value> for Elems {
    #[inline]
    fn from_iter<I: IntoIterator<Item = Value>>(it: I) -> Elems {
        Elems(Rep::Vals(slot(it.into_iter().collect())))
    }
}

/// `n` copies of `x`, or the allocator's refusal.
fn try_filled<T: Clone>(x: T, n: usize) -> Result<Vec<T>, std::collections::TryReserveError> {
    let mut v = Vec::new();
    v.try_reserve_exact(n)?;
    v.resize(n, x);
    Ok(v)
}

impl Elems {
    /// `arr_repeat(v, n)`: packed when `v` is a plain `Bool`, `Int` or
    /// `Float`, else `n` clones of `v`. `Err` is the allocator's refusal of
    /// the storage.
    pub(crate) fn repeat(v: &Value, n: usize) -> Result<Elems, std::collections::TryReserveError> {
        Ok(Elems(match v {
            Value::Bool(b) => Rep::Bools(try_filled(*b, n)?),
            Value::Int(i) => Rep::Ints(try_filled(*i, n)?),
            Value::Float(x) => Rep::Floats(try_filled(*x, n)?),
            _ => Rep::Vals(slot(try_filled(v.clone(), n)?)),
        }))
    }

    /// `arr_range`: the integers of `r` (empty when `r.end <= r.start`),
    /// packed, with `len` (the caller's count of them) reserved first. `Err`
    /// is the allocator's refusal of the storage.
    pub(crate) fn range(
        r: std::ops::Range<i64>,
        len: usize,
    ) -> Result<Elems, std::collections::TryReserveError> {
        let mut v = Vec::new();
        v.try_reserve_exact(len)?;
        v.extend(r);
        Ok(Elems(Rep::Ints(v)))
    }

    /// The number of elements.
    #[inline]
    pub(crate) fn len(&self) -> usize {
        match &self.0 {
            Rep::Vals(v) => v.len(),
            Rep::Bools(v) => v.len(),
            Rep::Ints(v) => v.len(),
            Rep::Floats(v) => v.len(),
        }
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Element `i`, the `Value` it stands for.
    #[inline]
    pub(crate) fn get(&self, i: usize) -> Option<Value> {
        match &self.0 {
            Rep::Vals(v) => v.get(i).cloned(),
            Rep::Bools(v) => v.get(i).map(|&b| Value::Bool(b)),
            Rep::Ints(v) => v.get(i).map(|&x| Value::Int(x)),
            Rep::Floats(v) => v.get(i).map(|&x| Value::Float(x)),
        }
    }

    /// The first element.
    #[inline]
    pub(crate) fn first(&self) -> Option<Value> {
        self.get(0)
    }

    /// The elements in order, as the `Value`s they stand for.
    #[inline]
    pub(crate) fn iter(&self) -> ElemsIter<'_> {
        ElemsIter {
            elems: self,
            at: 0,
            end: self.len(),
        }
    }

    /// The elements as a `[Value]`: borrowed for `Vals`, a temporary copy
    /// (O(n)) for a packed array. Read-only sites off the element paths.
    pub(crate) fn vals(&self) -> std::borrow::Cow<'_, [Value]> {
        match &self.0 {
            Rep::Vals(v) => std::borrow::Cow::Borrowed(&v[..]),
            _ => std::borrow::Cow::Owned(self.iter().collect()),
        }
    }

    /// The storage, borrowed.
    #[inline]
    pub(crate) fn view(&self) -> ElemsRef<'_> {
        match &self.0 {
            Rep::Vals(v) => ElemsRef::Vals(v),
            Rep::Bools(v) => ElemsRef::Bools(v),
            Rep::Ints(v) => ElemsRef::Ints(v),
            Rep::Floats(v) => ElemsRef::Floats(v),
        }
    }

    /// The storage, mutably borrowed (its length fixed).
    #[inline]
    pub(crate) fn view_mut(&mut self) -> ElemsMut<'_> {
        match &mut self.0 {
            Rep::Vals(v) => ElemsMut::Vals(v),
            Rep::Bools(v) => ElemsMut::Bools(v),
            Rep::Ints(v) => ElemsMut::Ints(v),
            Rep::Floats(v) => ElemsMut::Floats(v),
        }
    }

    /// Write element `i` (which must be in bounds): in place when the array
    /// is `Vals` or `v` is a plain scalar of its packed kind, else after
    /// converting it to `Vals`.
    #[inline]
    pub(crate) fn set(&mut self, i: usize, v: Value) {
        match (&mut self.0, v) {
            (Rep::Vals(xs), v) => xs[i] = v,
            (Rep::Bools(xs), Value::Bool(b)) => xs[i] = b,
            (Rep::Ints(xs), Value::Int(x)) => xs[i] = x,
            (Rep::Floats(xs), Value::Float(x)) => xs[i] = x,
            (_, v) => self.vals_mut_cold()[i] = v,
        }
    }

    /// Append `v`: packed when `v` is a plain scalar of the packed kind,
    /// else after converting the array to `Vals`.
    #[inline]
    pub(crate) fn push(&mut self, v: Value) {
        match (&mut self.0, v) {
            (Rep::Vals(xs), v) => xs.push(v),
            (Rep::Bools(xs), Value::Bool(b)) => xs.push(b),
            (Rep::Ints(xs), Value::Int(x)) => xs.push(x),
            (Rep::Floats(xs), Value::Float(x)) => xs.push(x),
            (_, v) => self.vals_mut_cold().push(v),
        }
    }

    /// [`Elems::set`], forgetting an old `Vals` element that is a scalar
    /// instead of running its drop glue ([`forget_scalar`]). It returns no
    /// old element, so the element writes inlined into `call_mut_op` (S5's
    /// leaf store) keep its wasm32 frame within its budget (cost only).
    #[inline]
    pub(crate) fn put(&mut self, i: usize, v: Value) {
        match (&mut self.0, v) {
            (Rep::Vals(xs), v) => forget_scalar(std::mem::replace(&mut xs[i], v)),
            (Rep::Bools(xs), Value::Bool(b)) => xs[i] = b,
            (Rep::Ints(xs), Value::Int(x)) => xs[i] = x,
            (Rep::Floats(xs), Value::Float(x)) => xs[i] = x,
            (_, v) => self.vals_mut_cold()[i] = v,
        }
    }

    /// Append every value of `it`, each as [`Elems::push`] would.
    pub(crate) fn extend<I: IntoIterator<Item = Value>>(&mut self, it: I) {
        let it = it.into_iter();
        if let Rep::Vals(v) = &mut self.0 {
            v.extend(it);
            return;
        }
        for v in it {
            self.push(v);
        }
    }

    /// Swap elements `i` and `j` (both in bounds), in place in any form.
    #[inline]
    pub(crate) fn swap(&mut self, i: usize, j: usize) {
        match &mut self.0 {
            Rep::Vals(v) => v.swap(i, j),
            Rep::Bools(v) => v.swap(i, j),
            Rep::Ints(v) => v.swap(i, j),
            Rep::Floats(v) => v.swap(i, j),
        }
    }

    /// The elements as a `Vec<Value>`, converting a packed array to `Vals`
    /// in place first (O(n), once; it stays `Vals`).
    pub(crate) fn vals_mut(&mut self) -> &mut Vec<Value> {
        if !matches!(self.0, Rep::Vals(_)) {
            let v: Vec<Value> = self.iter().collect();
            self.0 = Rep::Vals(slot(v));
        }
        match &mut self.0 {
            Rep::Vals(v) => v,
            _ => unreachable!("converted to Vals above"),
        }
    }

    /// [`Elems::vals_mut`] for a write or push of another kind into a packed
    /// array: out of line, so the element write paths inlined into the VM's
    /// dispatch loop stay small (cost only).
    #[cold]
    #[inline(never)]
    fn vals_mut_cold(&mut self) -> &mut Vec<Value> {
        self.vals_mut()
    }

    /// The elements, moved out as a `Vec<Value>`.
    #[cfg(not(target_arch = "wasm32"))]
    #[inline]
    pub(crate) fn into_vec(self) -> Vec<Value> {
        match self.0 {
            Rep::Vals(v) => v,
            Rep::Bools(v) => v.into_iter().map(Value::Bool).collect(),
            Rep::Ints(v) => v.into_iter().map(Value::Int).collect(),
            Rep::Floats(v) => v.into_iter().map(Value::Float).collect(),
        }
    }

    /// The elements, moved out as a `Vec<Value>` (an empty one is left to
    /// the `Drop`).
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub(crate) fn into_vec(mut self) -> Vec<Value> {
        std::mem::take(self.vals_mut())
    }

    /// Test probe (spec §4 S13): the name of the array's form, `Vals`,
    /// `Bools`, `Ints` or `Floats`. Test builds only.
    #[cfg(test)]
    pub(crate) fn form(&self) -> &'static str {
        match &self.0 {
            Rep::Vals(_) => "Vals",
            Rep::Bools(_) => "Bools",
            Rep::Ints(_) => "Ints",
            Rep::Floats(_) => "Floats",
        }
    }
}

/// [`Elems::iter`]: the elements in order, as owned `Value`s (a clone for
/// `Vals`, built from the bits for a packed form).
#[derive(Clone)]
pub struct ElemsIter<'a> {
    elems: &'a Elems,
    at: usize,
    end: usize,
}

impl Iterator for ElemsIter<'_> {
    type Item = Value;
    #[inline]
    fn next(&mut self) -> Option<Value> {
        if self.at < self.end {
            let v = self.elems.get(self.at);
            self.at += 1;
            v
        } else {
            None
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        let n = self.end - self.at;
        (n, Some(n))
    }
}

impl DoubleEndedIterator for ElemsIter<'_> {
    #[inline]
    fn next_back(&mut self) -> Option<Value> {
        if self.at < self.end {
            self.end -= 1;
            self.elems.get(self.end)
        } else {
            None
        }
    }
}

impl ExactSizeIterator for ElemsIter<'_> {}

impl<'a> IntoIterator for &'a Elems {
    type Item = Value;
    type IntoIter = ElemsIter<'a>;
    #[inline]
    fn into_iter(self) -> ElemsIter<'a> {
        self.iter()
    }
}

/// The payload of a `Some`, `Ok` or `Err`. A newtype only so that, on
/// wasm32, it can carry the AX-59 bounded `Drop`; it derefs to the value.
pub struct VBox(Box<Slot<Value>>);

/// By hand rather than derived: the derived clone goes through `Box`'s and
/// `ManuallyDrop`'s, adding frames per level to the (unbounded, value-depth)
/// recursion that clones a deep `Some` chain.
impl Clone for VBox {
    #[inline]
    fn clone(&self) -> VBox {
        VBox::new(Value::clone(self))
    }
}

impl std::ops::Deref for VBox {
    type Target = Value;
    #[inline]
    fn deref(&self) -> &Value {
        &self.0
    }
}

impl std::ops::DerefMut for VBox {
    #[inline]
    fn deref_mut(&mut self) -> &mut Value {
        &mut self.0
    }
}

impl std::fmt::Debug for VBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&**self, f)
    }
}

impl VBox {
    #[inline]
    pub(crate) fn new(v: Value) -> VBox {
        VBox(Box::new(slot(v)))
    }

    /// The value, moved out.
    #[cfg(not(target_arch = "wasm32"))]
    #[inline]
    pub(crate) fn into_inner(self) -> Value {
        *self.0
    }

    /// The value, moved out (`Unit` is left to the `Drop`).
    #[cfg(target_arch = "wasm32")]
    #[inline]
    pub(crate) fn into_inner(mut self) -> Value {
        std::mem::replace::<Value>(&mut self.0, Value::Unit)
    }
}

/// A channel's queue. A newtype only so that, on wasm32, it can carry the
/// AX-59 bounded `Drop`; it derefs to the queue.
#[derive(Default)]
pub struct Queue(Slot<VecDeque<Value>>);

slot_deref!(Queue => VecDeque<Value>);

impl std::fmt::Debug for Queue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&**self, f)
    }
}

/// A field of a container that holds values outside a newtype of its own
/// (a closure's code and capture cell), in a [`Slot`] it derefs through:
/// the owner's AX-59 bounded `Drop` drops it on wasm32.
#[derive(Clone, Default)]
pub struct Held<T>(Slot<T>);

impl<T> std::ops::Deref for Held<T> {
    type Target = T;
    #[inline]
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<T> std::ops::DerefMut for Held<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for Held<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&**self, f)
    }
}

impl<T> Held<T> {
    #[inline]
    pub(crate) fn new(x: T) -> Held<T> {
        Held(slot(x))
    }
}

/// AX-59 (wasm32): dropping a deeply nested value without recursing once per
/// level of data nesting. Drop glue for a deep value (an enum list, an array
/// of arrays, a `Some` chain, 100,000 levels deep) otherwise recurses through
/// the containers per level, outside every depth guard, and traps wasmtime's
/// default 512 KiB stack. Native keeps plain drop glue: its interpreter
/// thread's stack (sized from `max_depth`, at least 1 GiB) holds a
/// million-level drop.
///
/// Every container that holds a `Value` keeps it in a [`Slot`] (`Fields`,
/// `DictMap`, `Elems`, `VBox`, `Queue`, a closure's capture cell and code),
/// and its `Drop` hands the contents to `drop_bounded`, which runs plain glue
/// for the first `DROP_INLINE_DEPTH` levels and below that hands them to the
/// outermost drop, which frees them after its own glue has returned. So every
/// cycle of drop glue passes `drop_bounded`: `scripts/wasm_stack_budget.py`
/// checks that the drop component without it is acyclic, and that
/// `DROP_INLINE_DEPTH` passes of the component fit the headroom (class
/// `bounded (DROP_INLINE_DEPTH)`). Drop order is not observable: no `Value`
/// payload has a `Drop` with side effects.
#[cfg(target_arch = "wasm32")]
mod bounded_drop {
    use super::{ClosureCode, ClosureVal, DictMap, Elems, Fields, Queue, Rc, RefCell, Sym, VBox};
    use super::{Value, VecDeque};
    use std::cell::Cell;
    use std::collections::BTreeMap;
    use std::mem::{replace, take, ManuallyDrop};

    impl Drop for Fields {
        #[inline]
        fn drop(&mut self) {
            if !self.0.is_empty() {
                drop_bounded(take(&mut *self.0));
            }
        }
    }

    impl Drop for DictMap {
        #[inline]
        fn drop(&mut self) {
            if !self.0.is_empty() {
                drop_bounded(take(&mut *self.0));
            }
        }
    }

    /// Only `Vals` holds `Value`s; a packed form's scalars drop as plain
    /// glue with no recursion.
    impl Drop for Elems {
        #[inline]
        fn drop(&mut self) {
            if let super::Rep::Vals(v) = &mut self.0 {
                if !v.is_empty() {
                    drop_bounded(take(&mut **v));
                }
            }
        }
    }

    impl Drop for Queue {
        #[inline]
        fn drop(&mut self) {
            if !self.0.is_empty() {
                drop_bounded(take(&mut *self.0));
            }
        }
    }

    impl Drop for VBox {
        #[inline]
        fn drop(&mut self) {
            drop_bounded(replace::<Value>(&mut self.0, Value::Unit));
        }
    }

    /// The capture cell is an `Rc` of its own (R51 Fork 3): its bindings are
    /// dropped by `Captures`' `Drop` (env.rs) when the cell's last holder
    /// goes, so this drops only the code.
    impl Drop for ClosureVal {
        #[inline]
        fn drop(&mut self) {
            // SAFETY: `code` is a `ManuallyDrop` that only this `Drop` drops,
            // and nothing reads it after this line.
            drop_bounded(unsafe { ManuallyDrop::take(&mut self.code.0) });
        }
    }

    /// How many levels of container drops run as plain glue before the rest
    /// is deferred to the outermost drop. Sized so that this many levels of
    /// the drop cycle (its summed frames, as `scripts/wasm_stack_budget.py`
    /// reports them) fit the headroom `NEST_PER_DEPTH` leaves above the
    /// deepest guarded frame; the script reads this constant and asserts
    /// that.
    const DROP_INLINE_DEPTH: usize = 16;

    thread_local! {
        /// Levels of [`drop_bounded`] live.
        static DROP_DEPTH: Cell<usize> = const { Cell::new(0) };
        /// Whether [`DROP_LATER`] holds anything.
        static DROP_DEFERRED: Cell<bool> = const { Cell::new(false) };
        /// Contents handed from a drop [`DROP_INLINE_DEPTH`] levels down to
        /// the outermost one.
        static DROP_LATER: RefCell<Vec<DropLater>> = const { RefCell::new(Vec::new()) };
    }

    /// A container's contents whose drop was deferred (held only to be
    /// dropped).
    #[allow(dead_code)]
    pub(super) enum DropLater {
        Pairs(Vec<(Sym, Value)>),
        Map(BTreeMap<String, Value>),
        Elems(Vec<Value>),
        Queue(VecDeque<Value>),
        Value(Value),
        Code(Rc<ClosureCode>),
    }

    macro_rules! drop_later_from {
        ($($t:ty => $v:ident),* $(,)?) => {$(
            impl From<$t> for DropLater {
                fn from(x: $t) -> DropLater {
                    DropLater::$v(x)
                }
            }
        )*};
    }

    drop_later_from!(
        Vec<(Sym, Value)> => Pairs,
        BTreeMap<String, Value> => Map,
        Vec<Value> => Elems,
        VecDeque<Value> => Queue,
        Value => Value,
        Rc<ClosureCode> => Code,
    );

    /// Drop `x` (a container's contents) as plain glue unless
    /// [`DROP_INLINE_DEPTH`] drops are live; then defer it to the outermost
    /// drop, which frees the deferred contents after its own glue returns,
    /// each again at most [`DROP_INLINE_DEPTH`] levels deep. Never inlined,
    /// so the budget checker can name the drop cycle it bounds.
    #[inline(never)]
    pub(super) fn drop_bounded<T: Into<DropLater>>(x: T) {
        let d = DROP_DEPTH.get();
        if d >= DROP_INLINE_DEPTH {
            drop_defer(x.into());
            return;
        }
        DROP_DEPTH.set(d + 1);
        drop(x);
        if d == 0 && DROP_DEFERRED.get() {
            drop_drain();
        }
        DROP_DEPTH.set(d);
    }

    #[cold]
    #[inline(never)]
    fn drop_defer(x: DropLater) {
        DROP_LATER.with_borrow_mut(|l| l.push(x));
        DROP_DEFERRED.set(true);
    }

    #[cold]
    #[inline(never)]
    fn drop_drain() {
        while let Some(x) = DROP_LATER.with_borrow_mut(|l| l.pop()) {
            drop(x);
        }
        DROP_DEFERRED.set(false);
    }
}

/// A struct value: its type name and fields.
#[derive(Clone)]
pub struct StructVal {
    pub name: Sym,
    pub fields: Fields,
}

impl std::fmt::Debug for StructVal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Struct")
            .field("name", &sym_name(self.name))
            .field("fields", &self.fields)
            .finish()
    }
}

/// An enum variant value.
#[derive(Clone)]
pub struct EnumVal {
    pub enum_name: Sym,
    pub variant: Sym,
    pub fields: Fields,
}

impl std::fmt::Debug for EnumVal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Enum")
            .field("enum_name", &sym_name(self.enum_name))
            .field("variant", &sym_name(self.variant))
            .field("fields", &self.fields)
            .finish()
    }
}

impl Value {
    /// A struct value named `name`.
    pub(crate) fn record(name: Sym, fields: Fields) -> Value {
        Value::Struct(Rc::new(StructVal { name, fields }))
    }
}

/// A closure: its shared code and its capture cell. AX-40: the cell holds only
/// the lambda's free variables that were bound where it was created, not the
/// whole defining environment, and `code` (params + body) is shared by every
/// closure made from the same lambda, so making, cloning or calling a closure
/// never copies its body.
///
/// R51 spec Fork 3, closure representation A (TaintPort-5, -15, VmPin-5): the
/// capture cell is its own `Rc`, so a cast to a declared `fn(..) -> ..`
/// ([`conform::Contract`]) allocates a new `ClosureVal` that SHARES the cell
/// and carries the contract for that reference only. Identity (the capture
/// cell's taint and any address-keyed table) is the cell's address,
/// `Rc::as_ptr(&captured)`, the same for every reference. The cell's
/// per-binding taints live in [`Captures`] (S2); the seal marks are the typed
/// [`SealMarks`], never entries of the slot-ordered `captured` bindings (an
/// entry there would shift `param_base`).
#[derive(Debug)]
pub struct ClosureVal {
    pub code: Held<Rc<ClosureCode>>,
    /// The cell's bindings are in a [`Held`] inside [`Captures`]: on wasm32
    /// the cell's AX-59 bounded `Drop` (env.rs) drops them when its last
    /// holder goes.
    pub captured: Rc<RefCell<Captures>>,
    /// The `fn(..) -> ..` types this REFERENCE crossed, outermost = latest
    /// (veto interp.rs:104-112). `None` in an unsealed run.
    pub contract: Option<Rc<conform::Contract>>,
    pub seal: SealMarks,
}

impl ClosureVal {
    /// A closure with no contract and no seal marks.
    pub(crate) fn plain(code: Rc<ClosureCode>, captured: Captures) -> ClosureVal {
        ClosureVal {
            code: Held::new(code),
            captured: Rc::new(RefCell::new(captured)),
            contract: None,
            seal: SealMarks::default(),
        }
    }
}

/// AX-40's lend test on representation A (R51 Fork 3): the capture cell can
/// be lent to a call (moved out and back) only when nothing the body runs can
/// reach it. Every `ClosureVal` that shares the cell (a cast's wrapper) is a
/// way to reach it, so the cell must have exactly one holder, `cv`, and
/// `cv`'s references must be exactly the `private_refs` the caller counted.
#[inline(always)]
pub(crate) fn cell_is_private(cv: &Rc<ClosureVal>, private_refs: usize) -> bool {
    Rc::strong_count(&cv.captured) == 1 && Rc::strong_count(cv) == private_refs
}

/// Where a closure came from, in a sealed run (veto's capture-cell keys
/// `SEALED_CLOSURE_MARK`, `SEALED_FNVAL_MARK` and `PIN_FN_MARK`,
/// interp.rs:490-497). Always the default in an unsealed run.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SealMarks {
    /// A SEALED frame created the closure: it runs sealed wherever it is
    /// later called.
    pub sealed_origin: bool,
    /// A fn VALUE (`let g = f`) a sealed frame took of a candidate fn: a call
    /// of it from sealed code is not a call of an operator closure, and an
    /// operator closure may not be replaced by it.
    pub sealed_fnval: bool,
    /// The pin owner of the frame that created the closure (its `FnDef`
    /// address, 0 outside any fn): the closure's body is judged by the
    /// static pin analysis as that fn's code (veto `PIN_FN_MARK`).
    pub pin_owner: usize,
}

impl SealMarks {
    /// The candidate's closure (made in a sealed frame, or a sealed frame's
    /// fn value), as opposed to the operator's.
    pub(crate) fn is_candidates(self) -> bool {
        self.sealed_origin || self.sealed_fnval
    }
}

/// R13 native FFI: an opaque, affine native `Handle` = `{tag, payload}` where
/// `payload` indexes a per-module handle table — NEVER a raw pointer, so Axon
/// cannot forge a native pointer (I-4/I-11). `module`/`name` give nominal
/// identity (`gfx::Window` ≠ `gfx::Surface`); `resource` marks an affine handle
/// (consumed by a consuming native fn). The handle is opaque at the surface —
/// no field access, no arithmetic (E1803).
#[derive(Debug)]
pub struct HandleVal {
    pub module: String,
    pub name: String,
    pub payload: i64,
    pub resource: bool,
}

impl Value {
    /// A `Decimal` with mantissa `m`.
    pub(crate) fn decimal(m: i128) -> Value {
        Value::Decimal(Rc::new(m))
    }

    /// A tuple of `items`.
    pub(crate) fn tuple(items: Vec<Value>) -> Value {
        Value::Tuple(Rc::new(items.into()))
    }

    fn type_name(&self) -> String {
        match self {
            Value::Int(_) => "i64".into(),
            Value::SizedInt { ty, .. } => ty.name().into(),
            Value::Float(_) => "f64".into(),
            Value::Decimal(_) => "Decimal".into(),
            Value::Bool(_) => "bool".into(),
            Value::Str(_) => "str".into(),
            Value::Unit => "()".into(),
            Value::Array(_) => "[]".into(),
            Value::Struct(s) => sym_name(s.name).to_string(),
            Value::Enum(e) => sym_name(e.enum_name).to_string(),
            Value::Some(_) | Value::None => "Option".into(),
            Value::Ok(_) | Value::Err(_) => "Result".into(),
            Value::Closure(_) => "fn".into(),
            Value::Chan(_) => "chan".into(),
            Value::Tuple(_) => "tuple".into(),
            Value::Dict(_) => "dict".into(),
            Value::Handle(h) => format!("{}::{}", h.module, h.name),
        }
    }
}

// ── Non-local control flow ──────────────────────────────────────────────────

/// A non-`Ok` outcome of evaluation. Normal values flow as `Ok(Value)`; these
/// are the ways evaluation can stop short of producing a value in-place.
///
/// AX-46: every message is a `Box<str>` (two words, against a `String`'s
/// three), so a `Flow` is no wider than a `Value` plus its tag and every `R`
/// the evaluator returns is 24 bytes. Messages are built once, on the cold
/// path; nothing appends to them.
#[derive(Debug)]
pub enum Flow {
    /// `return <expr>` — unwind to the enclosing function boundary.
    Return(Value),
    /// `break` — exit the nearest loop.
    Break,
    /// `continue` — skip to the next loop iteration.
    Continue,
    /// A runtime panic (failed assert, type error, OOB index, …).
    Panic(Box<str>),
    /// An `@[verify]` / deploy-gate rejection — a *policy* failure (the artifact
    /// didn't meet its declared bound), distinct from a bug-crash. Mapped to a
    /// dedicated exit code (3) so CI can branch on "verification failed" vs "the
    /// program crashed" (BUG_HUNT #26).
    VerifyFailed(Box<str>),
    /// An `@[corrigible]` fn was called while the corrigibility latch was
    /// tripped (`corrigible_halt()`). The call is *refused* — the body never
    /// runs — and the latch never clears. A distinct flow (and exit code 4) so
    /// CI / a supervisor can tell "the kill-switch caught this" apart from a
    /// crash (101), a policy reject (3), or a static error (2). (R9)
    Halted(Box<str>),
    /// An AI-policy condition that stops the program but is NOT a crash: an
    /// `ai_*` call can't run because no model is reachable and no
    /// `@[ai(policy(fallback: …))]` is declared (E1300), the per-fn AI call
    /// budget is exhausted (E1301), or an unknown tier name is configured
    /// (E1302). These are user-actionable *policy/environment* mismatches with
    /// a clear fix in the message — not bugs like overflow/div0/OOB. A distinct
    /// flow (and exit code 5) so a supervisor can branch on "AI policy needs
    /// attention" specifically, exactly as @[verify]→3 and @[corrigible]→4 are
    /// carved out of the generic panic (101).
    AiPolicyUnreachable(Box<str>),
    /// `exit(code)` — terminate the process with `code`.
    Exit(i32),
    /// Phase 6: `resume(v)` inside an effect-handler arm — carries the value the
    /// handled operation should yield so the handled computation continues with
    /// it (tail-resumptive, single-shot). Raised by evaluating a `resume(..)`
    /// call and caught at the builtin-interception site that invoked the arm. If
    /// it escapes to a function/loop/top-level boundary, that is a `resume`
    /// outside a handler arm — treated as a panic (it should have been caught).
    Resume(Value),
    /// Phase 6 (multi-shot): a non-tail / multi-resume handler arm finished with
    /// `value` as the result of the whole `with` block. Unlike single-shot tail
    /// resume (which continues the suspended body via `Ok(Some(v))`), the replay
    /// path reifies the continuation by re-running the body, so the original
    /// suspended body is abandoned and its block value is `value`. Caught only by
    /// `eval_with_handler`; if it escapes, that is an interpreter bug.
    ///
    /// The `usize` is the handler-stack index of the frame whose arm ran: only
    /// the `with` block that pushed THAT frame may catch it. It used to be caught
    /// by the nearest `with` of any kind, so a `with` in candidate code could
    /// swallow a completion aimed at the operator's handler and keep running.
    /// Boxed (v0.22 carries `(Value, usize)` inline) so `Flow`'s widest payload
    /// stays one `Value`: the inline pair is 24 bytes, which moves `Flow`'s tag
    /// into `Value`'s niche and costs every `R` the evaluator matches (+4.9%
    /// instructions on collatz, +6.6% on the `loop_generic` repro). A
    /// completion is rare; the allocation is paid only by it.
    HandlerDone(Box<(Value, usize)>),
    /// Phase 6 (multi-shot): a handler arm tried to resume more than once (or
    /// resume non-tail) over a body that performs effects beyond the single
    /// intercepted operation — the replay-based continuation cannot soundly
    /// re-fire those effects. Surfaced as E1314 and mapped to a panic-class exit
    /// (the program is asking for true delimited continuations, which are
    /// deferred). Carries an explanatory message.
    MultiShotUnsound(Box<str>),
    /// Phase 5: a refinement-type PRECONDITION was violated at runtime — a value
    /// passed to a parameter `p: T where P` failed `P` when `_` was bound to it.
    /// The checker discharges this statically for constant args (E1209); for a
    /// non-constant arg the predicate becomes a runtime check (the spec's
    /// Z3-free `--proof-timeout 0` fallback). A distinct flow (exit code 6) so a
    /// supervisor can tell a caller's precondition breach apart from a @[verify]
    /// postcondition (3), a kill-switch (4), an ai-policy stop (5), and a generic
    /// bug-panic (101).
    RefineViolation(Box<str>),
    /// R12b: a kernel `Goal` exhausted its principal's budget mid-run. Not a
    /// crash — the goal hit the spend ceiling its principal was granted — so a
    /// distinct flow (exit code 7) lets a supervisor branch on "goal ran out of
    /// budget" apart from a @[verify] (3), kill-switch (4), ai-policy (5),
    /// refinement (6), and a generic panic (101). The partial best is preserved
    /// (queryable via `kernel_goal_best_score`). See R12b-kernel-goal.md (E1604).
    GoalBudgetExhausted(Box<str>),
    /// F5 (Phase 9): a builtin tried to perform an effect outside the ceiling
    /// declared by the active `Sandbox<P>` — e.g. an AI-emitted tool attempted
    /// `ai_complete` (Net effect) when the sandbox only permits `FS`. A distinct
    /// flow (exit code 8) so a supervisor can tell "the sandbox caught this" apart
    /// from a genuine crash (101), a policy rejection (3), or the kill-switch (4).
    SandboxViolation(Box<str>),
}

/// Process exit code for an `@[verify]` / deploy-gate rejection. Distinct from
/// 101 (genuine panic) and 2 (static check error) so pipelines can branch on a
/// policy rejection specifically (BUG_HUNT #26).
pub const VERIFY_FAILED_EXIT_CODE: i32 = 3;

/// Process exit code when an `@[corrigible]` call is refused by the tripped
/// corrigibility latch. Distinct from 101 (panic), 3 (verify), and 2 (static)
/// so a supervisor can branch on "the kill-switch fired" specifically. (R9)
pub const HALTED_EXIT_CODE: i32 = 4;

/// Process exit code for an AI-policy condition (E1300/E1301/E1302) — offline
/// with no fallback, AI budget exhausted, or an unknown tier. A user-actionable
/// policy/environment mismatch, not a crash; distinct from 101 (panic), 3
/// (verify), 4 (corrigible halt), and 2 (static) so a supervisor can branch on
/// "AI policy needs attention" specifically.
pub const AI_POLICY_EXIT_CODE: i32 = 5;

/// Process exit code for a runtime refinement-precondition violation — a
/// non-constant argument failed a parameter's `where` predicate. Distinct from
/// 101 (panic), 3 (verify postcondition), 4 (corrigible), 5 (ai-policy), and 2
/// (static) so a supervisor can branch on "a caller passed an out-of-contract
/// value" specifically. The spec's Z3-free runtime-check fallback (Phase-5 §4).
pub const REFINE_VIOLATION_EXIT_CODE: i32 = 6;

/// Process exit code when a kernel `Goal` exhausts its principal's budget mid-run
/// (R12b / E1604). Distinct from 101 (panic), 6 (refinement), 5 (ai-policy), 4
/// (corrigible), 3 (verify), 2 (static) so a supervisor can branch on "goal out
/// of budget" specifically. VERIFIED free in the exit-code table.
pub const GOAL_BUDGET_EXIT_CODE: i32 = 7;

/// Process exit code when `sandbox_run` catches a builtin attempting an effect
/// outside the sandbox's declared ceiling (F5 / Phase 9). Distinct from 101
/// (crash), 7 (goal-budget), 6 (refinement), 5 (ai-policy), 4 (kill-switch),
/// 3 (verify), 2 (static) so a supervisor can branch on "the sandbox caught this"
/// specifically. The tool call is refused; the sandbox and the enclosing program
/// continue with an `Err` result.
pub const SANDBOX_VIOLATION_EXIT_CODE: i32 = 8;

/// The enforcement ledger's block, and the shell's.
///
/// 2 through 15 belong to `governance/EXIT_CODES.md` — 3 verify, 4 kill-switch,
/// 6 refinement, 8 sandbox, 11 replay-divergence, up to its own "next free: 16".
/// 101 is a panic. Everything else in 0..=255 is a program's to use.
///
/// 126 and up are the SHELL's by convention (not-executable, not-found,
/// killed-by-signal-N) and are deliberately NOT reserved here: that is a
/// convention about how a shell reports its own failures, not a claim this
/// project makes on the number, and ordinary answers land there (a test summing
/// to 220 is not making a claim about signals).
const LEDGER_TOP: i64 = 15;

/// A status the program STATED, via `exit(n)`.
///
/// Stating a status is a deliberate act, so the ledger vocabulary is available:
/// a userland deploy gate that has decided to reject may say `exit(3)` and mean
/// the same "policy rejection" the `@[verify]` gate means. That is this repo's
/// own design (BUG_HUNT #26/#34 — every deploy-gate rejection is one exit
/// class), and taking the vocabulary away from userland would break it.
///
/// What is still refused is a value that is not a status at all: a status is one
/// byte, and `exit(3240)` would be observed as 168 — a number the program never
/// mentioned.
pub fn stated_exit_status(n: i64) -> (i32, Option<String>) {
    if (0..=255).contains(&n) {
        return (n as i32, None);
    }
    (
        1,
        Some(format!(
            "axon: exit({n}) is not a status — a status is one byte, so the caller would have \
             seen {}. Exiting 1 instead; pass a value in 0..=255.",
            n.rem_euclid(256)
        )),
    )
}

/// A value that fell out of `main`, which is an ANSWER, not a status.
///
/// `fn main() -> i64 { … result }` is the shape a program takes when it computes
/// something and hands it back. Passing that through to the process status
/// unchanged went wrong two ways, both measured rather than imagined:
///
/// * **Silent truncation.** `3240` was observed by the caller as `168`. The
///   number the program produced was not the number anyone saw, and nothing said
///   so. (A benchmark program computed its answer correctly, printed it,
///   returned it, and scored as a failure.)
/// * **Impersonating a guard.** A program whose answer happens to be 6 is
///   indistinguishable from a refinement violation, and 11 from a replay
///   divergence. A supervisor branches on that number precisely because it is
///   supposed to mean a guard fired; arithmetic could forge it.
///
/// Both become status 1 — which is what a nonzero return meant anyway — with the
/// reason on stderr. Ordinary values pass through, so `main() -> i64 { 49 }`
/// still exits 49; a program that MEANS a ledger code says so with `exit(n)`,
/// where the intent is explicit and is honoured.
///
/// MIRRORED in `axon-rt` (`__axon_main_status`) for the native engine. The two
/// are held together by `scripts/exit_code_parity.sh`, not by shared code — the
/// runtime crate deliberately depends on nothing.
pub fn returned_exit_status(n: i64) -> (i32, Option<String>) {
    let advice = "print it (`println(to_str(v))`) and return 0; if you MEAN a status, state it \
                  with `exit(n)`, which is honoured as written — see governance/EXIT_CODES.md";
    if n == 0 || n == 1 {
        return (n as i32, None);
    }
    if (2..=LEDGER_TOP).contains(&n) || n == RUNTIME_PANIC_EXIT_CODE as i64 {
        return (
            1,
            Some(format!(
                "axon: `main` returned {n}, and {n} is RESERVED — the exit-code ledger assigns \
                 it, so exiting with it would make this run indistinguishable from a guard \
                 firing. Exiting 1 instead; {advice}"
            )),
        );
    }
    if !(0..=255).contains(&n) {
        return (
            1,
            Some(format!(
                "axon: `main` returned {n}, which is not a status — a status is one byte, so the \
                 caller would have seen {}. Exiting 1 instead; {advice}",
                n.rem_euclid(256)
            )),
        );
    }
    (n as i32, None)
}

/// A panic is a crash, i.e. a bug — distinct from every enforcement code, which
/// is a guard doing its job. Named here so [`returned_exit_status`] can refuse to
/// let a computed value impersonate one.
pub const RUNTIME_PANIC_EXIT_CODE: i32 = 101;

type R = Result<Value, Flow>;

/// The edge of a frame, named in [`contain_frame`]'s refusal.
#[derive(Clone, Copy)]
pub(crate) enum FrameEdge<'a> {
    /// A named fn's call.
    Fn(&'a str),
    /// A closure's call.
    Closure,
    /// A refinement or `@[verify]` predicate.
    Predicate,
    /// An effect-handler arm.
    HandlerArm,
}

impl std::fmt::Display for FrameEdge<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FrameEdge::Fn(name) => write!(f, "`{name}`"),
            FrameEdge::Closure => f.write_str("a closure"),
            FrameEdge::Predicate => f.write_str("a predicate"),
            FrameEdge::HandlerArm => f.write_str("an effect-handler arm"),
        }
    }
}

impl FrameEdge<'_> {
    /// The panic text for a `break`/`continue` reaching this edge. A handler
    /// arm may sit lexically inside a loop, so "outside a loop" would be false
    /// there: the arm is its own frame and the loop is not its loop
    /// (S7-UnsealedParity-1).
    fn loop_control_escape(&self) -> String {
        match self {
            FrameEdge::HandlerArm => "`break`/`continue` cannot leave an effect-handler arm \
                                      (a loop around the `with` is not the arm's loop)"
                .to_string(),
            _ => format!("`break`/`continue` outside a loop in {self}"),
        }
    }
}

/// A FRAME EDGE is an allowlist, not a list of known escapes (Protected Check
/// Isolation 7/8/10/13). A call — named fn or closure — and a predicate
/// evaluation may end only with a value or with an ABORTIVE flow that
/// terminates or fails the program. Every flow that TRANSFERS control to some
/// enclosing construct (`return`, `break`, `continue`, `resume`) is meaningful
/// only inside the frame that raised it; leaving the frame is an error here,
/// never a jump in whatever loop or fn the surrounding code is running.
///
/// Escapes used to be closed one at a time — `break` out of a callee (FG-063/
/// 064), out of a predicate (FG-065), then `return` out of a predicate and a
/// smuggled `resume` closure (PCI candidate-1 review): each ended the caller's
/// loop or test early as a normal completion. A fn body's own `return` is its
/// value before this runs. The work is in [`frame_edge`], off the `Ok` path.
#[inline(always)]
pub(crate) fn contain_frame(r: R, edge: FrameEdge<'_>) -> R {
    match r {
        Ok(v) => Ok(v),
        Err(f) => Err(frame_edge(f, edge)),
    }
}

/// [`contain_frame`]'s classification of a flow reaching a frame edge. The
/// match has no wildcard, so a new `Flow` variant must be classified here
/// before the crate compiles.
#[cold]
#[inline(never)]
pub(crate) fn frame_edge(f: Flow, edge: FrameEdge<'_>) -> Flow {
    let msg = match f {
        // Transfers: meaningful only inside the frame that raised them.
        Flow::Return(_) => format!("`return` escaped {edge}"),
        Flow::Break | Flow::Continue => edge.loop_control_escape(),
        Flow::Resume(_) => format!("`resume` escaped {edge}"),
        // Abortive: they end or fail the program, so they may cross.
        // `HandlerDone` is the one cross-frame completion, and it is
        // addressed: only the `with` that installed its handler catches it.
        Flow::Panic(_)
        | Flow::VerifyFailed(_)
        | Flow::Halted(_)
        | Flow::AiPolicyUnreachable(_)
        | Flow::Exit(_)
        | Flow::HandlerDone(..)
        | Flow::MultiShotUnsound(_)
        | Flow::RefineViolation(_)
        | Flow::GoalBudgetExhausted(_)
        | Flow::SandboxViolation(_) => return f,
    };
    Flow::Panic(msg.into_boxed_str())
}

/// Loop control never leaves the frame it was written in: a `break`/`continue`
/// reaching an effect-handler arm's edge has no loop of its own there and is an
/// error AT that edge. The arm's other flows (`return`, `resume`, its value)
/// are its caller's business (`run_handler_arm`). Without this, candidate code
/// ended an operator test's loop before its assertions ran and the verdict was
/// signed (v0.22 G01 final reviews, FG-063/064/065).
#[inline(always)]
pub(crate) fn contain_loop_control(r: R, edge: FrameEdge<'_>) -> R {
    match r {
        Err(Flow::Break) | Err(Flow::Continue) => Err(frame_edge(Flow::Break, edge)),
        other => other,
    }
}

fn panic<T>(msg: impl Into<String>) -> Result<T, Flow> {
    Err(Flow::Panic(msg.into().into_boxed_str()))
}

/// Like `panic`, but for AI-policy conditions (E1300/E1301/E1302) that should
/// stop the program with the distinct [`AI_POLICY_EXIT_CODE`] (5) rather than
/// the generic crash code (101) — see [`Flow::AiPolicyUnreachable`].
fn ai_policy_err<T>(msg: impl Into<String>) -> Result<T, Flow> {
    Err(Flow::AiPolicyUnreachable(msg.into().into_boxed_str()))
}

// ── Lexical environment ──────────────────────────────────────────────────────

// `Env` and its binding storage `Bindings` (R51 S2.1, spec §4 Fork 2): the
// storage is private to interp/env.rs, so a binding (and in a sealed run its
// taint) is written only through a method that takes the taint.
mod env;
use env::*;

/// Whether `v` is a scalar: a value that owns nothing, so it has no drop
/// glue to run.
#[inline(always)]
fn is_scalar(v: &Value) -> bool {
    matches!(
        v,
        Value::Int(_)
            | Value::SizedInt { .. }
            | Value::Float(_)
            | Value::Bool(_)
            | Value::Unit
            | Value::None
    )
}

/// Drop a binding's value, forgetting a scalar (nothing to drop) instead of
/// running `Value`'s drop glue on it (cost only).
#[inline(always)]
fn forget_scalar(v: Value) {
    if is_scalar(&v) {
        std::mem::forget(v);
    } else {
        drop(v);
    }
}

// ── Sandbox state ────────────────────────────────────────────────────────────

/// F5 (Phase 9): one live sandbox entry — a named principal + the concrete set
/// of effects it allows at runtime. Created by `sandbox_create`; enforced by
/// `call_builtin` whenever `active_sandbox` points at this entry.
#[derive(Clone)]
struct SandboxEntry {
    /// The principal handle this sandbox is scoped to (for audit attribution).
    principal: i64,
    /// The concrete effects this sandbox permits (e.g. {"AI", "Net"}).
    allowed: std::collections::HashSet<String>,
    /// AUDIT T3: the SCOPE of those effects. `None` means "unscoped" — the
    /// effect is granted without a path/host restriction, which is the
    /// pre-existing `sandbox_create` behaviour and stays the default so old
    /// callers are unaffected. `Some(list)` restricts the effect to arguments
    /// matching one of the entries.
    ///
    /// Without this, `@[contained(fs: [write("./out/")], net: ["api.x.com"])]`
    /// enforced only "may write SOMEWHERE" / "may reach SOME host": the
    /// allowlists parsed, type-checked and were rendered to the approving human,
    /// then discarded before anything could enforce them.
    scope: SandboxScope,
}

/// Path/host restrictions attached to a sandbox. `None` = unscoped (grant the
/// effect with no argument restriction); `Some(v)` = the call argument must
/// match an entry, or it is a SandboxViolation.
#[derive(Default, Clone)]
struct SandboxScope {
    fs_read: Option<Vec<String>>,
    fs_write: Option<Vec<String>>,
    net: Option<Vec<String>>,
}

// ── Interpreter ──────────────────────────────────────────────────────────────

/// R51 §4 Fork 1, §7: whether this build carries the complete runtime seal
/// (taint, seal edges, PICK, pins). It was `false` from S0 to S5, when every
/// sealed run was refused whatever the engine; S6.1 flipped it to `true` in
/// the commit where the seal tests (T3-T8) are green. Setting it back to
/// `false` is the rollback: every sealed run is refused again.
pub const SEAL_RUNTIME_COMPLETE: bool = true;

/// R51 §3: the refusal every sealed run entry gives while
/// [`SEAL_RUNTIME_COMPLETE`] is `false`. The CLI prints it on stderr and exits 2.
pub const SEALED_RUN_REFUSED: &str = "sealed runs are refused on this build: the runtime seal is not complete (R51 SEAL_RUNTIME_COMPLETE)";

/// R51 §3: the refusal a sealed run gives when the VM is requested
/// (`AXON_ENGINE=vm` or `set_engine(Engine::Vm)`) on a build whose runtime
/// seal is complete. The CLI prints it on stderr and exits 2.
pub const SEALED_RUN_VM_REFUSED: &str = "a sealed run executes on the tree engine only (AXON_ENGINE=vm cannot carry the seal); unset AXON_ENGINE";

/// R51: `Err(message)` when a run with `sealed` code must not execute on this
/// build or with the engine asked for ([`EngineRefusal`]); `Ok` for every
/// unsealed run, without reading the engine inputs. [`Interp::build`] makes
/// the same decision for every run; `axon test` calls this first, so it
/// refuses before printing any result line.
pub fn sealed_run_refusal(sealed: bool) -> Result<(), String> {
    if !sealed {
        return Ok(());
    }
    vm::engine_at_build(true)
        .map(|_| ())
        .map_err(|refused| refused.to_string())
}

/// R51 S5, from v0.22 (Protected Check Isolation, RUNTIME sealing; C9
/// amendment 53): which definitions of the program a sealed (candidate)
/// module defines, by the ONE provenance rule (`resolver::span_in_sealed` of
/// the definition's span). Built once in [`Interp::build_with`] for a sealed
/// run; an unsealed run builds the empty default, which allocates nothing.
///
/// Frames carry provenance: a function by its definition's file, a closure by
/// the frame that CREATED it, a handler arm by the frame that installed the
/// `with` ([`Interp::with_frame`]).
///
/// Main's data model: a fn is keyed by its `FnDef` address (the key
/// `fn_of_def` uses); a global by its `Sym`, since main's `globals` table is
/// keyed by `Sym`, not by name text.
///
/// Readers by slice (R51 §11): `active` here (the VM compile refusal); the
/// sets, [`Interp::fn_is_sealed`] and the frame counters are read by the S2
/// taint and the S3 seal edges (`globals` by `seal_global`, `refines` by
/// `seal_refine`, `types` by `seal_type`, `operator_methods` by
/// `seal_method`); `fns` is read through `fn_is_sealed` by the S3 call edge.
#[derive(Default)]
pub(crate) struct Seal {
    pub(crate) active: bool,
    fns: std::collections::HashSet<usize>,
    globals: std::collections::HashSet<Sym>,
    refines: std::collections::HashSet<String>,
    /// Struct types defined in a sealed module: their whole-struct `where`
    /// runs under THEIR provenance, whoever constructs one.
    types: std::collections::HashSet<String>,
    /// Method names the OPERATOR's code defines (in its impls and traits).
    /// In operator code such a name means the operator's method: a call that
    /// would dispatch it to a sealed method (the receiver's runtime type is
    /// one the candidate chose) is refused (C9 round 4, PSV-1, amendment 53).
    operator_methods: std::collections::HashSet<String>,
}

// Test-only: the static dispatch rule (the pin analysis behind
// `seal_name_args`, `seal_dispatch`, `seal_width`, `seal_width_unary`) is
// switched off while a test of another seal layer runs, so that layer is
// judged by its own attack (v0.22 interp.rs:475). Per thread, not v0.22's
// process-global: main has no lock serialising sealed unit tests (R51 S4).
#[cfg(test)]
thread_local! { pub(crate) static DISPATCH_RULE_OFF: Cell<bool> = const { Cell::new(false) } }
// Test-only: the runtime taint rules apply (`Interp::t_rules`, v0.22
// interp.rs:480). Off by default in a unit test, so each older test judges
// one STATIC layer by its own attack and the taint cannot refuse the same
// attack first and hide a removed guard; the taint's own tests turn it on.
#[cfg(test)]
thread_local! { pub(crate) static TAINT_FORCE_ON: Cell<bool> = const { Cell::new(false) } }

impl Seal {
    /// The seal of `program` when `sealed` says which spans lie in the
    /// sealed set. An impl method is sealed when the method or its block is.
    fn of(program: &Program, sealed: &dyn Fn(crate::span::Span) -> bool) -> Seal {
        let mut seal = Seal {
            active: true,
            ..Seal::default()
        };
        for item in &program.items {
            match item {
                Item::FnDef(f) if sealed(f.span) => {
                    seal.fns.insert(f as *const FnDef as usize);
                }
                Item::ImplBlock(b) => {
                    for m in &b.methods {
                        if sealed(m.span) || sealed(b.span) {
                            seal.fns.insert(m as *const FnDef as usize);
                        }
                    }
                }
                Item::LetDef { name, span, .. } if sealed(*span) => {
                    seal.globals.insert(intern(name));
                }
                Item::RefineDef(r) if sealed(r.span) => {
                    seal.refines.insert(r.name.clone());
                }
                Item::TypeDef(t) if sealed(t.span) => {
                    seal.types.insert(t.name.clone());
                }
                _ => {}
            }
            match item {
                Item::ImplBlock(b) if !sealed(b.span) => {
                    for m in &b.methods {
                        if !sealed(m.span) {
                            seal.operator_methods.insert(m.name.clone());
                        }
                    }
                }
                Item::TraitDef(t) if !sealed(t.span) => {
                    for m in &t.methods {
                        seal.operator_methods.insert(m.name.clone());
                    }
                }
                _ => {}
            }
        }
        seal
    }
}

/// Restores the caller's `pin_fn` on drop.
struct PinGuard<'a> {
    cell: &'a Cell<usize>,
    prev: usize,
}
impl Drop for PinGuard<'_> {
    fn drop(&mut self) {
        self.cell.set(self.prev);
    }
}

/// Per-provenance kernel state (R51 S3, PCI runtime sealing; veto
/// interp.rs:708-842). Every table that holds objects addressed by HANDLE —
/// fibers, supervisors, principals, stores, LLM gateways, kernel goals,
/// sandboxes — plus the attribution and constraint state they drive. A sealed
/// (candidate) frame gets its OWN instance, so an operator handle does not
/// exist from a sealed frame, whatever id is guessed, and a candidate's fibers
/// never run in the operator's scheduler (PCI candidate-4 review: forged fiber
/// ids read the operator's reference result, reset its failed fibers, and a
/// predicted principal token spent its budget). Reached only through
/// [`Interp::k`]; an unsealed run only ever reaches `kernels[0]`.
struct Kernel {
    /// In-memory provenance store: `@[adaptive]` fn name → recorded return
    /// scores, in call order. Read by `goal_run` (mirrors `axon-rt`'s store).
    provenance: RefCell<HashMap<String, Vec<f64>>>,
    /// Per-call i64-prefix input tuple, in lock-step with `provenance`. The
    /// vec collects every leading i64 arg the fn took (length = how many
    /// of the fn's first args were i64). Empty when none were. Read by
    /// `goal_best_input` (returns the first dim) and `goal_best_inputs`
    /// (returns the full tuple), and used by the multi-arg coordinate-
    /// descent hill-climb to seed the next sweep.
    provenance_inputs: RefCell<HashMap<String, Vec<Vec<i64>>>>,
    /// Per-call f64-prefix input tuple, mirror of `provenance_inputs` for
    /// `@[adaptive] fn(f64, …) -> f64`. Read by `goal_best_input_f64` /
    /// `goal_best_inputs_f64`. Lets the optimizer cover continuous-domain
    /// problems (linear-regression weights, control parameters, etc.)
    /// without forcing the user to discretize via integer indices.
    provenance_inputs_f64: RefCell<HashMap<String, Vec<Vec<f64>>>>,
    /// R9 corrigibility latch. `corrigible_halt()` sets this to `true`; once
    /// set it never clears (there is intentionally no resume builtin). While
    /// set, every call to an `@[corrigible]` fn is refused — its body never
    /// runs — so the system cannot resist or reverse its own shutdown. A
    /// one-way latch is the whole safety property: a kill-switch you can turn
    /// back off is not a kill-switch.
    corrigible_halted: Cell<bool>,
    /// F3 (Phase 9): the name of the principal currently in scope for audit
    /// attribution. Set via `principal_activate(handle)` to associate a kernel
    /// principal with the execution context, so capability audit records carry the
    /// principal name rather than the opaque "root" default. Defaults to "root".
    current_principal: RefCell<String>,
    /// Active `subject_to` constraint fn name during `goal_run_constrained`
    /// (pillar-3 constrained search). When set, the optimizer scores an
    /// INFEASIBLE candidate as maximally distant so it is rejected; the real
    /// score is still recorded in provenance. `None` outside a constrained goal
    /// (so plain `goal_run` is byte-identical). See `apply_goal_constraint`.
    goal_constraint: RefCell<Option<String>>,
    /// Phase 7 (R12 Slice 1): the live principal-authority registry. The
    /// `principal_*` builtins mint/spend/authorize against it, so attenuation is
    /// enforced by the KERNEL (the registry), not just as userland values. A
    /// handle is a plain `i64` index. Empty until a program mints a root. The
    /// sealed kernel's registry draws its handle tokens from its own stream
    /// (`PrincipalRegistry::new_sealed`).
    principals: RefCell<crate::kernel::PrincipalRegistry>,
    /// Phase 7 (R12 Slice 2): the cooperative fiber scheduler. `scheduler_spawn`
    /// queues a (named fn, arg) fiber; `scheduler_run` runs the ready fibers in a
    /// seed-deterministic round-robin, catching a panicking fiber (recorded as
    /// failed, not a process abort). The interpreter owns the run loop (it has
    /// `call_fn`); the queue + ordering live in `kernel::Scheduler`.
    scheduler: RefCell<crate::kernel::Scheduler>,
    /// Phase 7 (R12 Slice 3): live supervisors, indexed by handle. Each oversees
    /// an ordered set of scheduler fibers and, when one fails, restarts the set
    /// its OTP strategy dictates — latching a halt (exit 4) on a crash loop.
    supervisors: RefCell<Vec<crate::kernel::Supervisor>>,
    /// Phase 7 (R12 Slice 4): durable stores, indexed by handle. Each is an
    /// in-memory `kernel::Store` (rebuilt by replaying its NDJSON log on open)
    /// plus the log path it appends applied ops to, so its value survives a fresh
    /// process and a retried op_id dedups cross-process under linearizable.
    stores: RefCell<Vec<(crate::kernel::Store, std::path::PathBuf)>>,
    /// Phase 7 (R12 Slice 5): principal-scoped LLM gateways, indexed by handle.
    /// Each mediates AI calls with per-token cost metering debited from its
    /// principal's budget (Slice 1), degrading to a fallback + latch on overrun.
    llm_gateways: RefCell<Vec<crate::kernel::LlmGateway>>,
    /// Phase 7 (R12b): principal-scoped `KernelGoal`s, indexed by handle. Each
    /// runs the existing optimizer (`run_goal`) scoped to a Slice-1 principal's
    /// budget, refusing to exceed it (E1604, exit 7). See R12b-kernel-goal.md.
    goals: RefCell<Vec<crate::kernel::KernelGoal>>,
    /// F5 (Phase 9): registered sandboxes, indexed by handle (0-based). Created
    /// by `sandbox_create`; the handle is the index into this vec.
    sandboxes: RefCell<Vec<SandboxEntry>>,
    /// F5 (Phase 9): the handle of the currently active sandbox (-1 = none).
    /// `sandbox_run` sets this before calling the user fn and restores it after.
    /// `call_builtin` reads it to gate effectful builtins.
    active_sandbox: Cell<i64>,
    /// The SEALED kernel's xorshift64 RNG state (`0` = not yet seeded). EVERY
    /// draw — `random_*`, the distribution samplers, `srand`, the `goal_*`
    /// searches and `@[forall]` input generation — goes through
    /// [`Interp::rng_next`] / [`Interp::rng_reseed`], i.e. through the kernel
    /// of the frame that is running, so a sealed candidate can neither
    /// advance, reseed nor observe the operator's stream, whichever builtin it
    /// uses (PSV-1). The OPERATOR kernel keeps main's process-global stream
    /// ([`next_rand_u64`]), so an unsealed run draws exactly the sequence it
    /// drew before R51, across every `Interp` of the process (one per `@[test]`
    /// fn); this cell stays `0` in the operator kernel. (Veto gave the operator
    /// kernel a per-`Interp` cell, which restarts the stream at each test.)
    rng: Cell<u64>,
    /// Whether this is the sealed (candidate) kernel. Its stream is its own
    /// cell, seeded by a one-way derivation, so drawing from it reveals
    /// nothing about the operator's seed — see [`sealed_rng_seed`].
    rng_sealed: bool,
}

impl Kernel {
    /// The kernel of one provenance: the operator's (`sealed == false`) or
    /// the sealed candidate's, both under the SAME ambient effect ceiling
    /// (`ambient`), so a sealed frame may narrow it but never widen it. The
    /// sandbox registry and the active-handle field are derived from it.
    fn new(sealed: bool, ambient: Vec<SandboxEntry>) -> Kernel {
        Kernel {
            provenance: RefCell::new(HashMap::new()),
            provenance_inputs: RefCell::new(HashMap::new()),
            provenance_inputs_f64: RefCell::new(HashMap::new()),
            corrigible_halted: Cell::new(false),
            current_principal: RefCell::new(
                std::env::var("AXON_PRINCIPAL")
                    .ok()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| "root".to_string()),
            ),
            goal_constraint: RefCell::new(None),
            principals: RefCell::new(if sealed {
                crate::kernel::PrincipalRegistry::new_sealed()
            } else {
                crate::kernel::PrincipalRegistry::new()
            }),
            // Scheduler order is a function of spawn order + AXON_SEED (R12 §5
            // determinism): derive the round-robin start offset from the seed.
            scheduler: RefCell::new(crate::kernel::Scheduler::new(rng_seed() as usize)),
            supervisors: RefCell::new(Vec::new()),
            stores: RefCell::new(Vec::new()),
            llm_gateways: RefCell::new(Vec::new()),
            goals: RefCell::new(Vec::new()),
            active_sandbox: Cell::new(if ambient.is_empty() { -1 } else { 0 }),
            sandboxes: RefCell::new(ambient),
            rng: Cell::new(0),
            rng_sealed: sealed,
        }
    }

    /// The next draw from THIS kernel's stream: the operator kernel's is the
    /// process-global one; the sealed kernel's is its own, seeded on first use.
    fn rng_next(&self) -> u64 {
        if !self.rng_sealed {
            return next_rand_u64();
        }
        let mut x = self.rng.get();
        if x == 0 {
            x = sealed_rng_seed(rng_seed());
        }
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng.set(x);
        x
    }

    /// `srand(n)`: reseed THIS kernel's stream only. `n == 0` maps to a
    /// non-zero sentinel so it does not read as "not yet seeded".
    fn rng_set(&self, n: i64) {
        if self.rng_sealed {
            self.rng.set((n as u64) | 1);
        } else {
            set_rand_seed(n);
        }
    }
}

/// The sealed kernel's initial RNG state: SHA-256 over a domain tag and the
/// operator seed. Deterministic under `AXON_SEED` (replay still reproduces a
/// candidate's draws), but one-way, so a candidate that reads its own stream
/// (xorshift64's output IS its state) learns nothing about the operator's.
fn sealed_rng_seed(operator_seed: u64) -> u64 {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"axon-sealed-rng/1\0");
    h.update(operator_seed.to_le_bytes());
    let d = h.finalize();
    let mut b = [0u8; 8];
    b.copy_from_slice(&d[..8]);
    u64::from_le_bytes(b) | 1
}

pub struct Interp<'p> {
    fns: HashMap<String, &'p FnDef>,
    #[allow(dead_code)]
    structs: HashMap<String, &'p TypeDef>,
    #[allow(dead_code)]
    enums: HashMap<String, &'p EnumDef>,
    /// `(type_name, method_name) → method def` from impl blocks.
    methods: HashMap<(String, String), &'p FnDef>,
    /// Module-level `let NAME = …` constant definitions, in source order.
    global_defs: Vec<(String, &'p Expr)>,
    /// Evaluated module-level constants (populated by [`Interp::init_globals`]).
    globals: FxHashMap<Sym, Value>,
    /// AX-18: the sym of every name-bearing AST node of the program, and each
    /// lambda's shared code — the evaluator's name lookups go through this
    /// instead of hashing or comparing names.
    res: Resolution,
    /// AX-18: every top-level fn and impl method with its parameter syms, so a
    /// call binds parameters without interning. Indexed by `fn_of_sym` (fns,
    /// by name) and `fn_of_def` (fns and methods, by `FnDef` address — stable
    /// for `'p`).
    fn_table: Vec<FnEntry<'p>>,
    fn_of_sym: FxHashMap<Sym, u32>,
    fn_of_def: FxHashMap<usize, u32>,
    /// R51 S3: the KERNEL tables, one per provenance: `kernels[0]` for the
    /// operator, `kernels[1]` for sealed (candidate) frames — selected by
    /// [`Interp::k`] from `frame_sealed`.
    kernels: [Kernel; 2],
    /// Current call-stack depth, bounded by `max_depth` so runaway recursion
    /// fails with a catchable panic rather than overflowing the (large but
    /// finite) interpreter thread stack and aborting the process.
    call_depth: Cell<usize>,
    /// The call depth of the `@[test]` frame being run (`0` = none), and
    /// whether THAT frame's body evaluated to its end. The one fact test
    /// completion is decided on (C9 round 3, PSV-3): not the returned value's
    /// tag, which a `?` on a type-confused `None` made read as a completion.
    test_frame_depth: Cell<usize>,
    test_body_finished: Cell<bool>,
    /// AX-56: on wasm32, the native-stack bytes the interpreter frames live
    /// right now have charged, bounded by `nest_limit` (see [`nest_cost`]).
    #[cfg(target_arch = "wasm32")]
    nest: Cell<usize>,
    /// AX-56: `max_depth × NEST_PER_DEPTH` bytes, resolved once at build.
    #[cfg(target_arch = "wasm32")]
    nest_limit: usize,
    /// Effective recursion ceiling for this run — `RECURSION_LIMIT` by default,
    /// or `AXON_MAX_DEPTH` (clamped) when set. Resolved once at build time so
    /// every `call_fn` sees a consistent value.
    max_depth: usize,
    /// R51 S5 (Protected Check Isolation, RUNTIME sealing): which
    /// definitions come from a sealed (candidate) module. `seal.active` is
    /// the one "this run executes sealed code" bit: computed once in
    /// [`Interp::build`] from the same input as the refuse-all pin, and
    /// `vm::compile::compile` refuses to build a body while it is set.
    pub(crate) seal: Seal,
    /// The static pin analysis (`interp/pin.rs`) of a sealed run; empty, and
    /// never computed, for an unsealed one.
    pins: pin::Pins,
    /// Frame provenance: whether the frame now running is a sealed
    /// (candidate) frame. Changed only by [`Interp::with_frame`].
    frame_sealed: Cell<bool>,
    /// The fn whose body is running, for the pin analysis (its `FnDef`
    /// address; a closure carries its creator's). 0 outside any fn.
    pin_fn: Cell<usize>,
    /// How many frames of each provenance are active (entered through
    /// [`Interp::with_frame`] and not yet left). `with_frame` is the ONLY
    /// place provenance changes, so these are exact. A sealed handler frame
    /// answers an operation only when no OPERATOR frame was entered after it
    /// was installed, and operator code never draws from the operator RNG
    /// while a sealed frame is below it (both S3).
    sealed_frames: Cell<usize>,
    operator_frames: Cell<usize>,
    /// R51 S2: the evaluator's taint state (`interp/taint.rs`): the
    /// accumulator, the control taints, the kernel and world cells, shared
    /// objects and module-level lets. Read and written only in a sealed run.
    taint: taint::Taint,
    /// R51 S3 (`interp/conform.rs`): the traits the program defines, and
    /// which `(type, trait)` pairs it implements — what a `dyn Trait` or a
    /// type parameter's bound is checked against (veto interp.rs:885-886).
    user_traits: std::collections::HashSet<String>,
    trait_impls: std::collections::HashSet<(String, String)>,
    /// Each refinement type's base type (veto interp.rs:888): a cast to a
    /// refinement is a cast to its base.
    refine_bases: HashMap<String, &'p crate::ast::AxonType>,
    /// The element types each channel OBJECT crossed (by address; the weak
    /// handle keeps the address from being reused while the entry exists).
    chan_contracts: RefCell<conform::ChanContracts>,
    /// The dicts the operator handed sealed code (by address): what each held
    /// (amendment 72 part 2, `interp/conform.rs`).
    dict_snaps: RefCell<HashMap<usize, conform::DictSnap>>,
    /// Bumped by every operator-side dict mutation: a snapshot from an older
    /// epoch is retaken at the next hand-over.
    dict_epoch: Cell<u64>,
    /// The impl block each method was defined in (by `FnDef` address), for
    /// reading its signature: `Self` and the impl's type parameters. Built
    /// for a sealed run only (its one reader is [`Interp::fn_cx`]).
    impl_of: HashMap<usize, &'p ImplBlock>,
    /// [`Interp::fn_cx`]'s per-fn signature environments.
    fn_cx_cache: RefCell<HashMap<usize, conform::Cx>>,
    /// R50: which engine runs fn bodies (`AXON_ENGINE`, or `set_engine` on a
    /// target without an environment). Read once, at build.
    engine: Engine,
    /// R50: `AXON_VM_TRACE=1`, read once at build. Only consulted under
    /// [`Engine::Vm`].
    vm_trace: bool,
    /// R50 S9: `AXON_VM_EAGER=1`, read once at build: compile every body on
    /// its first entry. Only consulted under [`Engine::Vm`].
    vm_eager: bool,
    /// The Axon function currently executing, whose name attributes builtin
    /// side effects (e.g. R3's `ai_call` provenance records) to their caller.
    /// Set on entry to `call_fn`, restored on exit. `None` at top level. A
    /// reference to the fn's definition, so a call copies and counts nothing
    /// (AX-54).
    current_fn: Cell<Option<&'p FnDef>>,
    /// R4/I-13 — the nearest ENCLOSING `@[agent]` fn on the call stack (not just
    /// the immediate fn). Set when entering an `@[agent]` fn and INHERITED through
    /// non-agent helpers, so a capability builtin called inside a helper of an
    /// agent is still logged to the agent's action trail (the un-opt-out-able
    /// audit can't be escaped by wrapping the I/O one call away). `None` outside
    /// any agent.
    enclosing_agent: RefCell<Option<String>>,
    /// F3: the goal/metric name currently being optimized by a `run_goal*` call,
    /// set for the duration of the optimization so an `ai_complete` invoked inside
    /// a metric evaluation can stamp the CAUSAL goal that triggered it into the
    /// audit trail (`axon trace --ai` cost-attribution per goal). `None` outside
    /// any goal optimization.
    current_goal: RefCell<Option<String>>,
    /// Per-call AI tier from a `tier:` named arg (R3b), set by `eval_call` for
    /// the duration of a single builtin dispatch. `ai_complete`'s tier
    /// resolution reads this first (step 1: per-call > policy > default).
    current_call_tier: RefCell<Option<String>>,
    /// Per callee sym: what a call by that name resolved to, once proven NOT
    /// to be a builtin — [`CALLEE_UNKNOWN`], [`CALLEE_NOT_FN`] (no user fn
    /// either), or `CALLEE_FN + i` for `fn_table[i]`. Filled by `eval_call` when
    /// `call_builtin` answered `Ok(None)` for a name whose pre-dispatch steps
    /// are provably inert (`builtin_dispatch_is_inert`), so later calls skip
    /// the ~600-arm builtin dispatch and every name lookup — the dominant
    /// per-call costs of a recursive user fn. A vector indexed by sym, so the
    /// lookup hashes nothing (AX-18).
    callees: RefCell<Vec<u32>>,
    /// R3c: count of `ai_complete` calls made by the current fn activation, used
    /// to enforce `@[ai(policy(budget: N))]`. Reset on entry to a fn that may
    /// meter (`FnEntry::ai_metered`), restored on exit (so the budget is
    /// per-activation, not global).
    ai_calls_this_fn: Cell<u64>,
    /// AX-54: emptied argument vectors of finished user-fn calls, reused by
    /// `eval_call` so a call does not allocate one.
    arg_bufs: RefCell<Vec<Vec<Value>>>,
    /// AX-54: emptied frames of finished user-fn calls, reused by
    /// `call_fn_entry` so a call does not allocate its bindings, marks and
    /// operand stack. Boxed so taking and returning a frame moves a pointer,
    /// not the frame.
    #[allow(clippy::vec_box)]
    env_pool: RefCell<Vec<Box<Env>>>,
    /// R50 S5: the most recently returned frame, kept out of `env_pool` so
    /// the common call (one frame in flight below the caller's) takes and
    /// returns it with a `Cell` swap instead of a `RefCell` borrow and a
    /// vector push/pop (cost only).
    frame_spare: Cell<Option<Box<Env>>>,
    /// R50 S11 (`vm::purefn`): the pure tier's per-`fn_table`-entry state
    /// (same indices), its registers and suspended frames (reused across
    /// calls), and whether a declined pure call is being replayed on the
    /// generic path (nested pure entry is off meanwhile).
    pure_slots: Box<[vm::PureSlot]>,
    pure_regs: Cell<Vec<i64>>,
    pure_frames: Cell<Vec<vm::PFrame>>,
    pure_replay: Cell<bool>,
    /// Phase-7 `cost_meter` / F4: cumulative AI spend across the whole run, in
    /// integer micro-dollars (µ$). Every `ai_complete` adds `tier.cost_micro(est
    /// tokens)` — the real per-token cost, stamped into the `ai_call` provenance
    /// (replacing the hardcoded 0). Read by the `ai_cost_spent()` builtin. This
    /// is per-TOKEN cost, distinct from R3c's per-CALL-count budget.
    ai_cost_micro: Cell<i64>,
    /// Fns already warned about by W1310, so the warning is emitted once per fn
    /// rather than once per CALL. The comment at the emit site always claimed
    /// "warn once"; it did not, so a `goal_run` search over an un-policied
    /// `@[adaptive]` fn printed one identical line per evaluation -- 8 lines for
    /// 8 evals, hundreds for a real search -- burying the diagnostics that were
    /// not repetitions. The text names only the fn, so every repeat after the
    /// first carries no information a reader did not already have.
    w1310_warned: RefCell<std::collections::HashSet<String>>,
    /// Tokens consumed so far by `ai_complete`, against [`Interp::token_budget`].
    ///
    /// Distinct from both sibling meters: `ai_cost_micro` is MONEY (per-token
    /// µ$ at the tier's rate) and R3c's `ai_calls_this_fn` counts CALLS within
    /// one fn. This is TOKENS across the whole run, because that is the unit an
    /// ambient operator cap is denominated in.
    tokens_used: Cell<i64>,
    /// Ambient run-level token cap from `AXON_BUDGET_TOKENS`, or `None` for no
    /// cap (the default). `Some(0)` is meaningful — it means "no AI at all" —
    /// so this is deliberately not a sentinel `0`.
    ///
    /// The var was set by `axon-guest-init` from the VM's MMDS policy and read
    /// by nothing, so a VM operator who capped a run's tokens got no cap: an
    /// inert control surface that reads as a safety mechanism. Enforced
    /// pre-dispatch beside the R3c per-fn budget gate.
    token_budget: Option<i64>,
    /// Phase 6: the stack of active effect-handler frames installed by enclosing
    /// `with handler { … } { body }` expressions. When a builtin carrying effect
    /// `E` is dispatched, the nearest frame with an `on E` arm intercepts it
    /// (tail-resumptive, single-shot). Pushed/popped around the handled body in
    /// `eval` of `Expr::WithHandler`. Interior-mutable like the other per-run
    /// state above.
    handlers: RefCell<Vec<HandlerFrame>>,
    /// Phase 6 (multi-shot resume): the active replay continuation, if the
    /// interpreter is currently re-running a handler's body to service a
    /// `resume(v)` call. `None` in the common case (no replay in flight). When
    /// `Some`, the builtin-interception site consumes one feed value for the
    /// handled effect instead of re-entering the arm (so the body runs straight
    /// through to its value), and flags any OTHER effect as E1314-unsound (a
    /// replay cannot re-fire side effects). See [`ResumeReplay`].
    resume_replay: RefCell<Option<ResumeReplay>>,
    /// Phase 6 (multi-shot resume): the stack of body+env contexts a handler arm
    /// is currently handling, so a `resume(v)` evaluated inside the arm knows
    /// WHICH suspended computation to replay. Pushed in `run_handler_arm` before
    /// the arm body runs, popped after. The top entry is the innermost handled
    /// operation. `resume(v)` replays `body` (the handled `with`-block body) with
    /// `v` fed at the intercepted op and returns the continuation's value.
    resume_ctx: RefCell<Vec<ResumeCtx>>,
    /// Phase 5: named refinement → its predicate Expr (binder `_`). Collected
    /// from `RefineDef` items (inline `where` on a param desugars to a synthetic
    /// named refinement during parsing). Drives the runtime precondition check in
    /// `call_fn`: when a parameter's type is one of these, the predicate is
    /// evaluated with `_` bound to the argument and a violation raises
    /// [`Flow::RefineViolation`]. Empty when the program has no refinements.
    refine_preds: HashMap<String, &'p Expr>,
    /// PROTOTYPE (RLM session option 2): `main`'s final top-level locals,
    /// captured after its body evaluates so AXON_DUMP_BINDINGS can persist a
    /// cell's mutated locals. Only populated when the env var is set.
    main_locals: RefCell<HashMap<String, Value>>,
    /// Phase 5 §4: obligations an SMT prover discharged for ALL inputs, so the
    /// matching runtime check is provably dead and may be elided. Empty by
    /// default (and always, unless `Interp::with_discharged` is used by a
    /// pipeline built with the `smt` feature), keeping the default run path
    /// byte-identical to pre-discharge behaviour.
    discharged: crate::verify::Discharged,
    /// R13 native FFI: the GPU-FREE `native::gfx` mock's in-process state — a
    /// per-module handle table + a frame counter. The shim is in-memory (no
    /// wgpu/winit/GPU); this is the interp-side realization of the dual-engine
    /// boundary (the codegen side links the `axon-rt` mock symbols, byte-
    /// identical observable behaviour for the value-returning calls).
    // Under `gfx-wgpu` the real backend is used instead, so the mock field is
    // unread there — keep it (cheap, always-constructed) and silence dead_code.
    #[cfg_attr(feature = "gfx-wgpu", allow(dead_code))]
    gfx_mock: RefCell<crate::native::GfxMock>,
    /// R22 domain-interop native modules (`native::modbus`/`fhir`/`fix`) —
    /// per-`Interp` backend state (one slab table per module). Interp-only; the
    /// network ones are codegen-refused (the `host_await`/native precedent).
    /// Gated to non-wasm targets (its tokio/reqwest deps don't build for the
    /// in-browser wasm32-unknown-unknown interpreter, R7c); on wasm a domain
    /// call is a clean "unavailable on this target" refusal.
    #[cfg(not(target_arch = "wasm32"))]
    domain: axon_domain::Registry,
    /// R13 slice 5: the REAL wgpu offscreen-render backend for `native::gfx`,
    /// present only under the `gfx-wgpu` feature. When set, `eval_native_call`
    /// routes `gfx::*` here (a real headless GPU render) instead of the mock —
    /// the same `dispatch` interface, a drop-in backend (one interface, two
    /// backends). The value-returning `read_pixel`/`frame_count` are byte-
    /// identical to the mock (same packing), so I-2 parity holds.
    #[cfg(feature = "gfx-wgpu")]
    gfx_real: RefCell<axon_gfx::GfxReal>,
}

/// One active effect-handler frame: the inline-handler arms in scope for the
/// body it wraps. Each arm intercepts one effect name. Captured at the `with`
/// site so the arm body closes over its defining environment.
struct HandlerFrame {
    /// `on E(binding) => body` arms, keyed by the effect name `E`.
    arms: Vec<HandlerArmRt>,
    /// Phase 6 (multi-shot resume): the `with`-block body this frame wraps, and
    /// the environment snapshot it ran in — so a non-tail / multi-shot arm can
    /// REPLAY the continuation (re-run the body, feeding the resume value at the
    /// intercepted op). Unused by the bare-tail-resume fast path.
    body: SrcRef<crate::ast::Expr>,
    env_snapshot: Snapshot,
    /// Provenance of the frame that installed this handler: its arms run
    /// under it (PCI runtime sealing).
    sealed: bool,
    /// `Interp::operator_frames` when this handler was installed. A SEALED
    /// frame may answer an operation only while the count is unchanged, i.e.
    /// no operator code lies between the `with` and the operation (PSV-1,
    /// C9 round 3). See [`Interp::handler_may_answer`].
    operator_frames: usize,
    /// `Interp::pin_fn` when this handler was installed: the fn that OWNS the
    /// `with` and so the arms' source text. The arm runs under THIS owner, not
    /// under whichever fn happened to perform the effect (C9 round 10, PSV-1,
    /// amendment 100): pin sites are keyed (owner, site text), so an arm run
    /// under the performer's owner read the performer's verdicts.
    pin_owner: usize,
}

/// A runtime handler arm: the arm as written, and a snapshot of the
/// environment where the handler was written (so the arm closes over it).
struct HandlerArmRt {
    arm: SrcRef<crate::ast::HandlerArm>,
    captured: Snapshot,
}

impl HandlerArmRt {
    /// The effect name this arm intercepts.
    fn effect(&self) -> &str {
        // SAFETY: a `HandlerArmRt` lives only in a `HandlerFrame`, which
        // `SrcRef`'s invariant keeps inside the `with` that borrows the arm.
        unsafe { &self.arm.get().effect }
    }
}

/// A pointer into the program source held by a [`HandlerFrame`] or a
/// [`ResumeCtx`], instead of a deep clone of the AST on every `with` entry
/// (whose recursion over the source, and the drop after, nothing charged
/// against the wasm32 stack budget, AX-56). Invariant: every frame and
/// resume context holding one lives only within the `eval_with_handler`
/// call that borrows the source it points into: the frame is pushed and
/// popped there, `run_handler_arm` splits frames off and restores them, and
/// pushes and pops its resume context, inside that call, with no early
/// return in between.
struct SrcRef<T>(std::ptr::NonNull<T>);

impl<T> Clone for SrcRef<T> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<T> Copy for SrcRef<T> {}

impl<T> SrcRef<T> {
    fn new(r: &T) -> Self {
        SrcRef(std::ptr::NonNull::from(r))
    }

    /// The source this points into.
    ///
    /// # Safety
    /// Only while the frame or resume context it came from is live (the
    /// type's invariant); the caller picks a lifetime no longer than that.
    unsafe fn get<'a>(self) -> &'a T {
        // SAFETY: the caller upholds the invariant above.
        unsafe { self.0.as_ref() }
    }
}

/// Phase 6 (multi-shot resume): state for one in-flight continuation replay. A
/// `resume(v)` in a handler arm reifies "the rest of the body after the
/// intercepted op" by RE-RUNNING the body from the top, with `v` fed at the
/// effect site instead of re-entering the handler. This makes the continuation a
/// first-class, repeatedly-callable thing in a tree-walking interpreter without
/// a CPS rewrite — the arm may `resume` as many times as it likes (multi-shot).
///
/// Soundness boundary: a replay that re-encounters ANY effect (the handled one a
/// second time, or a different effect) cannot soundly re-execute it — the side
/// effect already happened on the original pass. The first feed is the resume
/// value; a second effect hit during the same replay is E1314
/// ([`Flow::MultiShotUnsound`]). So the supported multi-shot subset is "a body
/// that performs exactly one effect, then is pure" — retries, backtracking
/// search, and `a + b` over two resumes all fit; an effect-after-resume does not.
struct ResumeReplay {
    /// The handled effect name this replay feeds (only this effect is fed).
    effect: String,
    /// The value to yield at the (single) intercepted op during this replay.
    feed: Value,
    /// Whether the feed has been consumed yet (the first hit consumes it; a
    /// second effect hit in the same replay is the unsound case → E1314).
    consumed: bool,
    /// The taint of `feed` (the `resume(..)` argument, with the control it was
    /// evaluated under): read when the replay yields it, so a value derived from
    /// a candidate-chosen resume value stays tainted inside the replayed body
    /// (amendment 117, loop finding 28). `0` in an unsealed run.
    feed_t: u8,
    /// Provenance of the handler whose arm armed this replay, and
    /// `Interp::operator_frames` when it did. The feed answers an operation
    /// only under the same rule as a live handler frame
    /// ([`Interp::handler_may_answer`]): a sealed arm's `resume(v)` never
    /// becomes the result of an operation the operator's code performs.
    sealed: bool,
    operator_frames: usize,
}

/// Phase 6 (multi-shot resume): the suspended computation a handler arm is
/// servicing — the `with`-block body and the environment it ran in. A
/// `resume(v)` evaluated in the arm replays this body (feeding `v` at the
/// intercepted op) and returns the continuation's value to the arm, so the arm
/// can resume again (multi-shot). Cloned cheaply (the body is a [`SrcRef`]).
#[derive(Clone)]
struct ResumeCtx {
    /// The handled effect name (the op the body performs once, fed by resume).
    effect: String,
    /// The `with`-block body to replay on each resume.
    body: SrcRef<crate::ast::Expr>,
    /// The environment snapshot the body originally evaluated in.
    env_snapshot: Snapshot,
    /// Provenance of the handler frame whose arm is servicing this body.
    sealed: bool,
    /// The pin owner of the fn that installed the handler: the replayed body is
    /// that fn's own text, so it replays under that owner (amendment 100).
    pin_owner: usize,
}

/// Default max interpreter call depth before a graceful "recursion limit"
/// panic. The debug-build `eval` frame is large (~128 KB/call), so a 1 GiB
/// thread stack overflows around ~8000 frames; this limit fires first, turning
/// runaway recursion into a catchable panic instead of a process-aborting
/// overflow. Overridable via `AXON_MAX_DEPTH` — see [`resolve_max_depth`].
#[cfg(not(target_arch = "wasm32"))]
const RECURSION_LIMIT: usize = 6_000;

/// On wasm32 the interpreter runs on the single linear-memory stack (no OS
/// thread to size up — see [`on_deep_stack`]), so the recursion guard must
/// trip before either stack the module runs on is exhausted, or a deep
/// recursion *traps* the module instead of producing the graceful "recursion
/// limit" panic native gives. Two stacks bound it: the 64 MiB linear stack the
/// build sets (`.cargo/config.toml` `-zstack-size`), and the wasm engine's own
/// native stack, which holds every wasm frame (wasmtime's default
/// `max-wasm-stack` is 512 KiB). The second is reached first: one recursion
/// level is several interpreter frames (16–704 bytes each), and under
/// wasmtime's default the shallowest of the five chains in
/// `tests/fixtures/vm_depth/` completes depth 157 (tree-walker, fn -> closure
/// -> fn, debug build, depth limit raised; measured 2026-10-09), above 1.2×
/// the guard. A call level
/// nested inside expressions (a `with` body, `1 + (1 + (..))`, a builtin
/// calling back into a closure, nested lambdas) takes more frames per level,
/// so a native-stack budget ([`NEST_PER_DEPTH`], [`nest_cost`]) backs the
/// depth guard. `scripts/vm_wasm_depth.sh` requires every chain, engine and
/// profile to complete 1.2× the guard under the default stack, and every
/// deeper probe to panic rather than trap (AX-56). This is the bounded,
/// documented host divergence (R7 §4.3 / BUG_HUNT #28): deep recursion fails
/// the same *way*, at a lower *depth*, on wasm; `AXON_MAX_DEPTH` raises it
/// for a host that gives the module a larger stack.
#[cfg(target_arch = "wasm32")]
const RECURSION_LIMIT: usize = 128;

/// AX-56: the wasm32 native-stack budget, in bytes, is `max_depth ×
/// NEST_PER_DEPTH`: 448 KiB at the default depth limit, which leaves 64 KiB
/// of wasmtime's default 512 KiB `max-wasm-stack` for the frames outside the
/// recursion (the run's base frames, the bounded leaf work above the deepest
/// guarded frame). Dropping a nested value is kept inside that headroom: it
/// runs at most `DROP_INLINE_DEPTH` levels of drop glue (`bounded_drop`,
/// compilebench AX-59), and `host_await_val` no longer copies its payload on
/// wasm32 (`SendValue::request`, AX-61). Comparing or formatting a deep
/// value still recurses per level outside the budget. The front end runs
/// before the interpreter, on the same 448 KiB, with the parser's nesting
/// limit sized to fit (`MAX_EXPR_DEPTH` 224, AX-60).
/// `scripts/wasm_stack_budget.py` checks the drop and parser bounds against
/// the frames of the build.
#[cfg(target_arch = "wasm32")]
const NEST_PER_DEPTH: usize = 3584;

/// AX-56: the stack, in bytes, each guarded interpreter function charges
/// against the wasm32 budget while it is live (`nest_guard!`,
/// [`Interp::enter_nest`]). Every recursion through `Interp::eval` (the
/// strongly connected component holding it in the module's call graph), and
/// the bytecode compiler's recursion (`COMPILE_EXPR`, `COMPILE_STMT`), passes
/// a guarded function, and each constant covers that function's frame plus
/// the deepest chain of unguarded functions it can call before the next
/// guarded one. Frames are the ones wasmtime's Cranelift gives on x86-64
/// (`sub rsp` plus return address and frame pointer), the largest over the
/// function's instances, per profile; `0` where the profile inlines it. Each
/// constant is named after its function. `scripts/wasm_stack_budget.py`
/// (run by `scripts/vm_wasm_depth.sh`) recomputes the components, the frames
/// and the tails from the build under test and fails when a constant is too
/// small, an unguarded cycle exists, an indirect call widens a component, or
/// another recursion reachable from `eval` is not classified in its `ALLOW`.
#[cfg(target_arch = "wasm32")]
pub(super) mod nest_cost {
    /// `(debug, release)` frame bytes, picked by the profile being built.
    const fn b(debug: usize, release: usize) -> usize {
        if cfg!(debug_assertions) {
            debug
        } else {
            release
        }
    }
    pub const EVAL_ARM: usize = b(192, 304);
    pub const EVAL_BLOCK: usize = b(112, 0);
    pub const EVAL_BINOP: usize = b(144, 0);
    pub const EVAL_WITH_HANDLER: usize = b(144, 0);
    pub const RUN_HANDLER_ARM: usize = b(192, 0);
    pub const EVAL_CALL: usize = b(352, 320);
    pub const DISPATCH_CALL: usize = b(176, 0);
    pub const DISPATCH_NAMED: usize = b(144, 192);
    pub const IMPL_METHOD: usize = b(96, 192);
    pub const CALL_MUT: usize = b(208, 336);
    pub const CALL_FN: usize = b(112, 128);
    pub const CALL_FN_ENTRY: usize = b(112, 224);
    pub const CALL_FN_IN: usize = b(160, 208);
    pub const CALL_FN_FRAME: usize = b(272, 576);
    pub const CALL_LOCAL_CLOSURE: usize = b(112, 128);
    pub const CALL_CLOSURE_ARG: usize = b(112, 272);
    pub const CALL_CLOSURE_FRAME: usize = b(256, 336);
    pub const CALL_BUILTIN: usize = b(496, 720);
    pub const RUN_BODY_VM: usize = b(160, 208);
    /// The tree's fn body charges what a VM level does above its own
    /// (smaller) frame: the VM's fn body plus its op loop ([`RUN`], which a
    /// level whose call sits in a `Tree` op of a compiled body passes
    /// through), so the VM panics no shallower than the tree there (spec
    /// R50 §4 Recursion).
    pub const RUN_BODY_TREE: usize = RUN_BODY_VM + RUN;
    /// The `@[test]` frame's body runner charges what `run_body_tree` does.
    pub const RUN_TEST_BODY: usize = RUN_BODY_VM + RUN;
    /// A compile frame and its unguarded tail (`compile`, `Vec` growth).
    /// Live only while one compile runs, never under the recursion; the
    /// checker fits them like the in-cycle constants.
    pub const COMPILE_BODY: usize = b(1504, 1024);
    pub const COMPILE_LAMBDA: usize = b(1616, 976);
    /// The tree's lambda body charges what a compiled VM lambda level does
    /// above `call_closure_frame`, its op loop ([`RUN`]), so the VM
    /// panics no shallower than the tree there (spec R50 §4 Recursion).
    pub const RUN_LAMBDA_TREE: usize = RUN;
    pub const RUN: usize = b(336, 544);
    pub const CALL_FAST_ARG: usize = b(224, 336);
    pub const CALL_FAST_INLINE: usize = b(240, 416);
    pub const CALL_MUT_OP: usize = b(320, 432);
    pub const ASSIGN_IN_PLACE: usize = b(176, 224);
    pub const BIND_LET: usize = b(160, 224);
    pub const CHAN_METHOD: usize = b(128, 96);
    pub const DISPATCH_RESUME: usize = b(224, 208);
    pub const EVAL_NATIVE_CALL: usize = b(256, 320);
    pub const FINISH_RECORD: usize = b(160, 432);
    pub const RUN_LOOP_BODY: usize = b(112, 128);
    pub const BUILTIN_SCHEDULER_RUN_ONCE: usize = b(128, 304);
    pub const CALL_CLOSURE: usize = b(112, 128);
    pub const CALL_FN_GOAL: usize = b(480, 928);
    pub const FINISH_CALL_COLD: usize = b(224, 496);
    pub const FLATTEN_PLACE: usize = b(128, 160);
    pub const CALL_FAST: usize = b(224, 320);
    pub const CALL_FAST_LOCAL_INT_SLOW: usize = b(176, 176);
    pub const CALL_MUT_GENERAL: usize = b(176, 304);
    pub const CALL_UNPROVEN: usize = b(192, 272);
    pub const RECORD_OP: usize = b(160, 160);
    pub const STORE_BIN_SLOW: usize = b(192, 192);
    pub const STORE_LOCAL_STACK_SLOW: usize = b(192, 224);
    pub const GOAL_EVAL_HOLDOUT: usize = b(128, 400);
    pub const RUN_GOAL: usize = b(672, 1024);
    pub const RUN_GOAL_CATEGORICAL: usize = b(416, 640);
    pub const RUN_GOAL_MULTISTART: usize = b(672, 1152);
    pub const RUN_GOAL_RANDOM: usize = b(208, 368);
    pub const RUN_GOAL_WARM: usize = b(656, 1040);
    pub const COMPILE_EXPR: usize = b(816, 688);
    pub const COMPILE_STMT: usize = b(800, 672);
    /// `P(dist op k)` (R51 moved it out of `eval_call`'s frame, which no
    /// longer carries it as an unguarded tail).
    pub const EVAL_PROB_PRED: usize = b(208, 224);
    /// The sealed run's instances (R51): the `::<true>` instance of a
    /// function compiled once per run kind, and the functions only a sealed
    /// run enters. An unsealed run charges none of them.
    pub const EVAL_ARM_SEALED: usize = b(336, 656);
    pub const EVAL_BLOCK_SEALED: usize = b(128, 0);
    pub const EVAL_BINOP_SEALED: usize = b(336, 0);
    pub const EVAL_CALL_SEALED: usize = b(400, 0);
    pub const DISPATCH_CALL_SEALED: usize = b(176, 304);
    pub const DISPATCH_NAMED_SEALED: usize = b(176, 0);
    pub const BIND_LET_SEALED: usize = b(160, 256);
    pub const RUN_LOOP_BODY_SEALED: usize = b(112, 128);
    pub const CHAN_METHOD_SEALED: usize = b(128, 112);
    pub const ASSIGN_IN_PLACE_SEALED: usize = b(176, 0);
    pub const CALL_FN_FRAME_SEALED: usize = b(272, 592);
    pub const CALL_CLOSURE_FRAME_SEALED: usize = b(352, 416);
    pub const IMPL_METHOD_SEALED: usize = b(160, 256);
    pub const EVAL_TAINTED: usize = b(144, 144);
    pub const CALL_FN_SEALED: usize = b(288, 176);
    pub const CALL_MUT_SEALED: usize = b(192, 384);
    pub const RUN_ARM_SEALED: usize = b(480, 144);
    pub const STRUCT_REFINE_SEALED: usize = b(288, 112);
}

/// Hard ceiling on the configurable recursion limit. `AXON_MAX_DEPTH` is
/// clamped to this so a user can't set a value so high the native stack
/// overflows *before* the guard fires (which would reintroduce the very
/// process-abort the guard exists to prevent). Paired with the stack-size
/// scaling in [`stack_size_for_depth`]: at this ceiling the thread stack is
/// [`MAX_STACK_BYTES`] of reserved address space.
const MAX_DEPTH_CEILING: usize = 1_000_000;

/// Native stack budget per interpreter call level, used to size the thread
/// stack so the [`resolve_max_depth`] guard trips before a real overflow. Only
/// call depth is counted, so a level whose call sits inside deeply nested
/// expressions can still use more than this (each nesting level is one more
/// evaluator frame). R51 (S7-UnsealedParity-10): 256 KiB → 320 KiB, with
/// [`MIN_STACK_BYTES`] 1 GiB → 1.25 GiB, because splitting the evaluator per
/// run kind grew the unsealed native frames (release `eval_arm::<false>`
/// 1024 B, was `eval` 912 B; stack per level +3.5–12.3% release, +6.1–10.5%
/// debug on the tree engine, up to +19.3% debug on the VM, measured
/// 2026-10-11): ×1.25 keeps every level the base tree reached reachable. The
/// reserve is address space; only the pages a run touches are resident.
const STACK_BYTES_PER_FRAME: usize = 320 * 1024;

/// Minimum interpreter thread stack, so shallow runs keep generous headroom
/// regardless of the depth setting (the historical 1 GiB floor, scaled with
/// [`STACK_BYTES_PER_FRAME`] so the crossover stays at 4096 levels).
const MIN_STACK_BYTES: usize = 1280 * 1024 * 1024;

/// Largest interpreter thread stack: the reserve 5864c423 gave
/// [`MAX_DEPTH_CEILING`] (1,000,000 × 256 KiB, ~244 GiB). Past ~800,000
/// levels the ×1.25 per-level reserve would exceed it, and Linux's heuristic
/// overcommit refuses a single mapping larger than RAM + swap, so the thread
/// would fail to spawn where 5864c423 ran (S7-UnsealedParity-10). Capped,
/// every depth reserves at least what 5864c423 did and never more than its
/// largest reserve. Saturating: wasm32's 32-bit `usize` (no thread there).
const MAX_STACK_BYTES: usize = MAX_DEPTH_CEILING.saturating_mul(256 * 1024);

/// Resolve the effective recursion limit: `AXON_MAX_DEPTH` if set to a positive
/// integer (clamped to [`MAX_DEPTH_CEILING`]), else [`RECURSION_LIMIT`]. A
/// malformed or zero value falls back to the default rather than failing the
/// run — the env var is a convenience lever, not load-bearing.
fn resolve_max_depth() -> usize {
    max_depth_from_env(std::env::var("AXON_MAX_DEPTH").ok().as_deref())
}

/// Pure core of [`resolve_max_depth`]: maps an optional raw env value to the
/// effective ceiling. Split out so the clamping/fallback logic is unit-testable
/// without mutating process-global environment state.
fn max_depth_from_env(raw: Option<&str>) -> usize {
    match raw {
        Some(s) => match s.trim().parse::<usize>() {
            Ok(n) if n > 0 => n.min(MAX_DEPTH_CEILING),
            _ => RECURSION_LIMIT,
        },
        None => RECURSION_LIMIT,
    }
}

/// Thread stack size that keeps the recursion guard ahead of a real overflow
/// for the given depth: `depth × per-frame budget`, floored at the historical
/// 1 GiB so we never shrink below the previous default. Unused on wasm32 (no
/// OS threads — `on_deep_stack` runs on the single main stack there).
#[cfg_attr(target_arch = "wasm32", allow(dead_code))]
fn stack_size_for_depth(depth: usize) -> usize {
    depth
        .saturating_mul(STACK_BYTES_PER_FRAME)
        .clamp(MIN_STACK_BYTES, MAX_STACK_BYTES)
}

/// Restores the call depth and the caller's `current_fn` when a `call_fn`
/// frame unwinds (any path), so builtin side effects (R3 `ai_call`
/// provenance) are attributed to the nearest enclosing Axon function even
/// across nested calls.
struct CallGuard<'a, 'p> {
    interp: &'a Interp<'p>,
    prev_fn: Option<&'p FnDef>,
}
impl Drop for CallGuard<'_, '_> {
    #[inline(always)]
    fn drop(&mut self) {
        self.interp.current_fn.set(self.prev_fn);
        let depth = &self.interp.call_depth;
        depth.set(depth.get().saturating_sub(1));
    }
}

/// AX-56: gives back what an `enter_nest::<W>` charged when the frame
/// unwinds (any path).
#[cfg(target_arch = "wasm32")]
pub(super) struct NestGuard<'a, const W: usize>(&'a Cell<usize>);
#[cfg(target_arch = "wasm32")]
impl<const W: usize> Drop for NestGuard<'_, W> {
    #[inline(always)]
    fn drop(&mut self) {
        self.0.set(self.0.get() - W);
    }
}

/// [`NestGuard`] for [`Interp::enter_nest_t`]: gives back `S` in the
/// `::<true>` (sealed) instance, `F` in the `::<false>` one.
#[cfg(target_arch = "wasm32")]
pub(super) struct NestGuardT<'a, const T: bool, const F: usize, const S: usize>(&'a Cell<usize>);
#[cfg(target_arch = "wasm32")]
impl<const T: bool, const F: usize, const S: usize> NestGuardT<'_, T, F, S> {
    const W: usize = if T { S } else { F };
}
#[cfg(target_arch = "wasm32")]
impl<const T: bool, const F: usize, const S: usize> Drop for NestGuardT<'_, T, F, S> {
    #[inline(always)]
    fn drop(&mut self) {
        self.0.set(self.0.get() - Self::W);
    }
}

/// AX-56: charges the enclosing interpreter function's native frame
/// (`nest_cost::$cost`) against the wasm32 stack budget until it returns, or
/// returns the recursion-limit panic when the budget is spent, running
/// `$spent` first (to release arguments the function owns). Expands to
/// nothing off wasm32. Measured 2026-10-09: this `?` form leaves smaller
/// unoptimised frames than an `Option` guard with `let .. else`. A function
/// compiled once per run kind (`f<const T: bool>`, R51) names both of its
/// instances' constants: `nest_guard!(self, if T { F_SEALED } else { F })`.
macro_rules! nest_guard {
    ($interp:ident, if $t:ident { $sealed:ident } else { $cost:ident }) => {
        #[cfg(target_arch = "wasm32")]
        let _nest = $interp.enter_nest_t::<
            $t,
            { $crate::interp::nest_cost::$cost },
            { $crate::interp::nest_cost::$sealed },
        >()?;
    };
    ($interp:ident, if $t:ident { $sealed:ident } else { $cost:ident }, $spent:block) => {
        #[cfg(target_arch = "wasm32")]
        let _nest = match $interp.enter_nest_t::<
            $t,
            { $crate::interp::nest_cost::$cost },
            { $crate::interp::nest_cost::$sealed },
        >() {
            Ok(g) => g,
            Err(flow) => {
                $spent
                return Err(flow);
            }
        };
    };
    ($interp:ident, $cost:ident) => {
        #[cfg(target_arch = "wasm32")]
        let _nest = $interp.enter_nest::<{ $crate::interp::nest_cost::$cost }>()?;
    };
    ($interp:ident, $cost:ident, $spent:block) => {
        #[cfg(target_arch = "wasm32")]
        let _nest = match $interp.enter_nest::<{ $crate::interp::nest_cost::$cost }>() {
            Ok(g) => g,
            Err(flow) => {
                $spent
                return Err(flow);
            }
        };
    };
}

/// The evaluated arguments of a call, in order, as the call path
/// ([`Interp::dispatch_named`], [`Interp::call_fn_entry`]) consumes them:
/// a `Vec` from [`Interp::take_args`] (the tree-walker), or the top of a
/// bytecode activation's operand stack ([`StackTail`]), which a call that
/// binds them to a fn's parameters reads in place (cost only, AX-54).
trait CallArgs {
    fn count(&self) -> usize;
    /// The arguments; a binder moves each out, leaving a `Unit`.
    fn values(&mut self) -> &mut [Value];
    /// Finished with the arguments: drop what is left of them and release
    /// the storage.
    fn done(self, interp: &Interp<'_>);
    /// The arguments as an owned vector (for a callee that takes one).
    fn into_vec(self, interp: &Interp<'_>) -> Vec<Value>;
}

impl CallArgs for Vec<Value> {
    #[inline(always)]
    fn count(&self) -> usize {
        self.len()
    }
    #[inline(always)]
    fn values(&mut self) -> &mut [Value] {
        self
    }
    #[inline(always)]
    fn done(mut self, interp: &Interp<'_>) {
        self.clear();
        interp.recycle_args(self);
    }
    #[inline(always)]
    fn into_vec(self, _: &Interp<'_>) -> Vec<Value> {
        self
    }
}

/// The arguments of a call as the values of `st` from index `at` on (R50:
/// the operand stack of the calling activation). Dropping it truncates `st`
/// to `at`, dropping them where dropping their vector did.
struct StackTail<'a> {
    st: &'a mut Vec<Value>,
    at: usize,
}

impl Drop for StackTail<'_> {
    #[inline(always)]
    fn drop(&mut self) {
        self.st.truncate(self.at);
    }
}

impl CallArgs for StackTail<'_> {
    #[inline(always)]
    fn count(&self) -> usize {
        self.st.len() - self.at
    }
    #[inline(always)]
    fn values(&mut self) -> &mut [Value] {
        &mut self.st[self.at..]
    }
    #[inline(always)]
    fn done(self, _: &Interp<'_>) {}
    #[inline]
    fn into_vec(self, interp: &Interp<'_>) -> Vec<Value> {
        let mut v = interp.take_args(self.count());
        v.extend(self.st.drain(self.at..));
        v
    }
}

/// R50 S4: the arguments of a closure call, which
/// [`Interp::call_closure_owned_by`] moves into the parameters' scope: a
/// call's vector is drained (its owner reuses it), and a builtin's array of
/// one or two values goes into the frame with no vector in between, so a
/// builtin -> closure call allocates no argument vector (cost only).
pub(super) trait ClosureArgs {
    fn count(&self) -> usize;
    /// Hand each argument, in order and with its index, to `bind`.
    fn bind(self, bind: impl FnMut(usize, Value));
}

impl ClosureArgs for &mut Vec<Value> {
    #[inline(always)]
    fn count(&self) -> usize {
        self.len()
    }
    #[inline(always)]
    fn bind(self, mut bind: impl FnMut(usize, Value)) {
        for (i, v) in self.drain(..).enumerate() {
            bind(i, v);
        }
    }
}

impl ClosureArgs for [Value; 1] {
    #[inline(always)]
    fn count(&self) -> usize {
        1
    }
    #[inline(always)]
    fn bind(self, mut bind: impl FnMut(usize, Value)) {
        let [a] = self;
        bind(0, a);
    }
}

impl ClosureArgs for [Value; 2] {
    #[inline(always)]
    fn count(&self) -> usize {
        2
    }
    #[inline(always)]
    fn bind(self, mut bind: impl FnMut(usize, Value)) {
        let [a, b] = self;
        bind(0, a);
        bind(1, b);
    }
}

/// The panic of a call to `f` with `got` arguments for a different number of
/// parameters.
#[cold]
#[inline(never)]
fn arity_mismatch(f: &FnDef, got: usize) -> R {
    panic(format!(
        "{}: expected {} args, got {}",
        f.name,
        f.params.len(),
        got
    ))
}

/// The panic of calling `c`, a value that is not a closure.
#[cold]
#[inline(never)]
fn not_callable(c: &Value) -> R {
    panic(format!("value of type {} is not callable", c.type_name()))
}

/// The panic of a call to a lambda of `want` parameters with `got`
/// arguments.
#[cold]
#[inline(never)]
fn lambda_arity_mismatch(want: usize, got: usize) -> R {
    panic(format!("lambda: expected {want} args, got {got}"))
}

/// Bind a closure call's arguments as the parameters of `code` in the
/// fresh scope above `base` (the cell's bindings), each with taint `t(i)`.
/// Slotted parameters are the next bindings, so they are pushed in place, as
/// `define_var` would (cost only).
#[inline(always)]
fn bind_closure_params(
    frame: &mut Env,
    code: &ClosureCode,
    base: usize,
    args: impl ClosureArgs,
    t: impl Fn(usize) -> u8,
) {
    if code.param_base.map(|b| b as usize) == Some(base) {
        frame.reserve(code.params.len());
        args.bind(|i, v| frame.push_var(code.params[i], v, t(i)));
    } else {
        args.bind(|i, v| {
            let slot = code.param_base.map_or(NAMED, |b| b + i as u32);
            frame.define_var(code.params[i], slot, v, t(i));
        });
    }
}

/// Whether the return boundary ([`sum_return`]) can ask anything of `v`:
/// only a struct (a soft wrapper) or a `Result`/`Option` constructor can be
/// asked anything. `Struct`..=`Err` is one contiguous tag range (`Enum`,
/// inside it, returns as is), so this is one compare, and it reads only the
/// value, never the entry (cost only, R51 S0b).
#[inline(always)]
fn ret_needs_check(v: &Value) -> bool {
    matches!(
        v,
        Value::Struct(_)
            | Value::Enum(_)
            | Value::Some(_)
            | Value::None
            | Value::Ok(_)
            | Value::Err(_)
    )
}

/// The return boundary, one fn on every return path of both engines: what
/// `entry`'s declared return type ([`FnEntry::ret`]) asks of `result`
/// ([`scalar_return`]), tested on the value's tag first
/// ([`ret_needs_check`]).
#[inline(always)]
fn sum_return(entry: &FnEntry<'_>, result: Value) -> R {
    if ret_needs_check(&result) {
        return scalar_return(entry, result);
    }
    Ok(result)
}

/// [`sum_return`] for a struct or a `Result`/`Option` constructor.
///
/// Soft typing: a fn declared `-> T` (a plain scalar) whose body produces
/// an `Uncertain<T>`/`Temporal<T>` unwraps to the inner value — the same
/// rule as a plain-T parameter. Without this, `fn f() -> i64 { uncertain }`
/// leaked the struct and `f() + 1` silently produced 0. A fn declared
/// `-> Uncertain<T>`/`-> Temporal<T>` keeps it; a str/struct/tuple/soft-
/// wrapper return is left untouched.
///
/// A value whose constructor contradicts the declared `Result`/`Option`
/// return type is a runtime type confusion (an untyped `dict_get` yields a
/// free type variable, so a stored `None` type-checks as a `Result`):
/// refused here, the one boundary every return crosses on both engines,
/// instead of letting a caller's `?` read it as the other type (v0.22 C9
/// round 3, PSV-3).
#[inline(always)]
fn scalar_return(entry: &FnEntry<'_>, result: Value) -> R {
    match (entry.ret, &result) {
        (RetCheck::Scalar, Value::Struct(_)) => {
            if let Some(inner) = value::soft_inner(&result) {
                return Ok(inner);
            }
        }
        (RetCheck::Result, Value::Some(_) | Value::None)
        | (RetCheck::Option, Value::Ok(_) | Value::Err(_)) => {
            return Err(sum_confusion(entry, &result));
        }
        _ => {}
    }
    Ok(result)
}

#[cold]
#[inline(never)]
fn sum_confusion(entry: &FnEntry<'_>, result: &Value) -> Flow {
    let declared = if entry.ret == RetCheck::Result {
        "Result"
    } else {
        "Option"
    };
    Flow::Panic(
        format!(
            "`{}` is declared to return {declared} but produced {} — a runtime type confusion",
            entry.def.name,
            value::display(result)
        )
        .into_boxed_str(),
    )
}

/// Argument `a` as `entry`'s parameter `i` binds it. Soft typing:
/// `Uncertain<T>` is compatible with a plain-`T` parameter (the checker
/// allows it). If the declared param type is NOT itself a soft wrapper but
/// the argument IS one, unwrap to the inner value so the body sees a plain
/// `T` (else `x * 2` on the struct silently produced 0). Confidence/horizon
/// dropped at this T-typed boundary. R19 Slice B: coerce Int→SizedInt when
/// the declared param type is a non-i64 integer width, so arithmetic inside
/// the callee's body uses width-correct ops (completeness, I-9).
/// In a sealed run (`SEALED`) a `SizedInt` of another width is left for the
/// declared-type cast that follows ([`coerce_to_sized`]).
#[inline(always)]
fn param_value<const SEALED: bool>(entry: &FnEntry<'_>, i: usize, a: Value) -> Value {
    match entry.param_coerce[i] {
        // Only a struct can be a soft wrapper: anything else binds as is
        // when there is no width to coerce to (cost only, R50 S5).
        (_, None) if !matches!(a, Value::Struct(_)) => a,
        (unwrap_soft, width) => param_value_coerced::<SEALED>(unwrap_soft, width, a),
    }
}

/// [`param_value`]'s soft unwrap and sized-int coercion.
#[inline(never)]
fn param_value_coerced<const SEALED: bool>(
    unwrap_soft: bool,
    width: Option<IntWidth>,
    a: Value,
) -> Value {
    let a = if unwrap_soft {
        value::soft_inner(&a).unwrap_or(a)
    } else {
        a
    };
    match width {
        Some(width) => coerce_to_sized::<SEALED>(a, width),
        None => a,
    }
}

/// Like `CallGuard`'s `current_fn` restore but for an `Option<String>` cell —
/// used for the `enclosing_agent` save/restore (R4/I-13 transitive agent
/// attribution).
struct FnNameOptGuard<'a> {
    cell: &'a RefCell<Option<String>>,
    prev: Option<String>,
}
impl Drop for FnNameOptGuard<'_> {
    fn drop(&mut self) {
        *self.cell.borrow_mut() = self.prev.take();
    }
}

/// R3c: saves the caller's `ai_calls_this_fn` count and restores it on drop, so
/// each fn activation meters its own `ai_complete` calls against its own
/// `@[ai(policy(budget))]` (the budget is per-activation, not global).
struct AiBudgetGuard<'a> {
    cell: &'a Cell<u64>,
    prev: u64,
}
impl Drop for AiBudgetGuard<'_> {
    fn drop(&mut self) {
        self.cell.set(self.prev);
    }
}

/// Run `f` on a thread with a large stack. The tree-walking interpreter uses a
/// lot of native stack per call, so an 8 MB main stack overflows at only a few
/// hundred frames; this lets reasonably deep recursion run, while the
/// `RECURSION_LIMIT` guard backstops truly runaway recursion with a clean panic.
///
/// On `wasm32` there are no OS threads (`std::thread::spawn` traps with
/// "invalid stack size"), so we run `f` directly on the single wasm stack. The
/// `RECURSION_LIMIT` / `AXON_MAX_DEPTH` guard still fires its graceful panic
/// before a wasm stack overflow, so deep recursion fails the same observable
/// way as native — only the (impossible-on-wasm) extra thread is dropped.
/// This is R7 §4.3: the one host touchpoint that must change for the
/// interpreter to run identically in the browser / under wasmtime.
#[cfg(not(target_arch = "wasm32"))]
fn on_deep_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    // Size the stack to the (possibly user-raised) recursion limit so the
    // RECURSION_LIMIT guard always trips before a real overflow (BUG_HUNT #28).
    let stack = stack_size_for_depth(resolve_max_depth());
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(stack)
            .spawn_scoped(s, f)
            .expect("spawn interpreter thread")
            .join()
            .expect("interpreter thread panicked")
    })
}

/// wasm32 has no OS threads — run on the single main stack. The depth guard
/// (`RECURSION_LIMIT` / `AXON_MAX_DEPTH`) still backstops runaway recursion.
#[cfg(target_arch = "wasm32")]
fn on_deep_stack<T>(f: impl FnOnce() -> T) -> T {
    f()
}

// ── In-process stdout capture (R10 G1 observable tuple) ──────────────────────
//
// The interpreter normally writes `print`/`println` straight to the process
// stdout. The R10 verification harness needs to compare a program's *observable
// output* before and after a candidate compiler pass — in-process, over a whole
// corpus, deterministically. A thread-local sink lets `run_program_capturing`
// redirect that output into a buffer without spawning a subprocess per program.
// When the sink is `None` (the normal case) output goes to real stdout, so this
// is zero-overhead and invisible to every existing run.
thread_local! {
    static OUTPUT_SINK: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Route an interpreter `print`/`println` write: into the capture buffer if one
/// is active on this thread, else to real stdout (the normal path).
fn emit_stdout(s: &str, newline: bool) {
    OUTPUT_SINK.with(|sink| {
        let mut b = sink.borrow_mut();
        if let Some(buf) = b.as_mut() {
            buf.push_str(s);
            if newline {
                buf.push('\n');
            }
        } else {
            use std::io::Write;
            let mut out = std::io::stdout();
            let _ = out.write_all(s.as_bytes());
            if newline {
                let _ = out.write_all(b"\n");
            }
            let _ = out.flush();
        }
    });
}

/// Run `program`, capturing its stdout into a buffer instead of the process
/// stdout, and return the **observable tuple** `(exit_code, stdout)`. This is
/// the R10 G1 oracle's comparison input: a candidate pass is correct iff this
/// tuple is identical for the original and transformed program on every corpus
/// member. Runs on the deep stack like `run_program`. Not thread-safe with
/// concurrent captures on the same thread (the sink is per-thread, restored on
/// return).
pub fn run_program_capturing(program: &Program) -> (i32, String) {
    on_deep_stack(|| {
        // Install a fresh capture buffer, restoring any prior one on exit.
        let prev = OUTPUT_SINK.with(|s| s.replace(Some(String::new())));
        // Unmapped: the oracle compares what two programs PRODUCE. Mapping first
        // would collapse distinct results into 1 and call a rewrite equivalent
        // when it is not.
        let code = run_program_inner(program, crate::verify::Discharged::default(), false);
        let captured = OUTPUT_SINK.with(|s| s.replace(prev)).unwrap_or_default();
        (code, captured)
    })
}

/// Parse-and-run convenience: returns the **process exit status** — what the
/// caller of `axon run` observes, with `stated_exit_status` /
/// `returned_exit_status` applied.
pub fn run_program(program: &Program) -> i32 {
    on_deep_stack(|| run_program_inner(program, crate::verify::Discharged::default(), true))
}

/// Like [`run_program`], but returns the value the program PRODUCED rather than
/// the status a caller would observe.
///
/// For reading a result out of a program — a test asserting on a computed value,
/// or anything comparing two runs. Guard outcomes (verify, refinement, sandbox …)
/// still come back as their ledger codes; only `main`'s own return and `exit(n)`
/// are left alone.
pub fn run_program_unmapped(program: &Program) -> i32 {
    on_deep_stack(|| run_program_inner(program, crate::verify::Discharged::default(), false))
}

/// Phase 5 §4: run with a set of SMT-discharged obligations installed, so the
/// interpreter elides the runtime checks Z3 proved ∀-inputs. Identical to
/// [`run_program`] with an empty set.
/// Describe a value's SHAPE — its structure, never its contents.
///
/// `AXON_FOR_RLM.md` §5 / atlas `RLM_MODE_SPEC.md` §11 arm A. A session tells
/// the model which names are bound; the measured failure is that names alone
/// leave it guessing what they *hold*, so it indexes a `[Row]` as if it were a
/// str and the reuse produces a wrong answer confidently.
///
/// The describer answers that without executing anything and without leaking a
/// value. Every branch here is written to that rule:
///
///   * scalars give a type name and never a number,
///   * `str` gives a LENGTH and never its characters,
///   * containers give an element description and a length,
///   * a struct gives its field NAMES — schema, which is the point — and its
///     field types, never their values.
///
/// A field name is the one branch that could leak, if a namespace were keyed by
/// data rather than by schema. That is a real risk and the reason the caller is
/// expected to scan the emitted text for dataset values rather than trust this
/// doc comment.
///
/// Depth-limited to two levels: deeper nesting would make the description grow
/// with the DATA, and the whole claim of a shape inventory is that it is a
/// per-turn constant that tracks schema instead.
pub fn value_shape(v: &Value) -> String {
    fn go(v: &Value, d: usize) -> String {
        match v {
            // A length only at the TOP level. Nested, it would be ambiguous
            // (`Row { region: str, len=5 }` reads as a field named `len`) and it
            // would track the DATA — every element of a `[str]` has its own
            // length, so the description would grow with the array instead of
            // staying a schema.
            Value::Str(s) if d == 0 => format!("str, len={}", s.chars().count()),
            Value::Str(_) => "str".to_string(),
            Value::Array(items) => {
                if let (Some(first), true) = (items.first(), d < 2) {
                    format!("[{}], len={}", go(&first, d + 1), items.len())
                } else {
                    format!("[], len={}", items.len())
                }
            }
            Value::Tuple(items) => {
                if d < 2 {
                    let inner: Vec<String> = items.iter().map(|i| go(&i, d + 1)).collect();
                    format!("({})", inner.join(", "))
                } else {
                    format!("tuple, len={}", items.len())
                }
            }
            Value::Struct(s) => {
                let name = sym_name(s.name);
                if d < 2 {
                    let inner: Vec<String> = s
                        .fields
                        .by_name()
                        .into_iter()
                        .map(|(k, v)| format!("{k}: {}", go(v, d + 1)))
                        .collect();
                    format!("{name} {{ {} }}", inner.join(", "))
                } else {
                    name.to_string()
                }
            }
            Value::Dict(dd) => {
                let b = dd.borrow();
                let n = b.len();
                // Keys are schema ONLY when the dict is a record. A dict built by
                // GROUPING is keyed by the data itself, so printing the key set
                // leaks dataset values into what is supposed to be a value-free
                // shape — `arr_group_by(rows, region)` would emit the regions.
                // The two cases are indistinguishable here, so neither prints its
                // keys: the key TYPE and the count are the schema, and the value
                // shape (which tells the model whether `dict_get` hands back an
                // i64 or a [Row]) is kept because it carries no data.
                if let (Some(k), true) = (b.keys().next(), d < 2) {
                    format!(
                        "dict, len={n}, keys are str, values are {}",
                        go(&b[k], d + 1)
                    )
                } else {
                    format!("dict, len={n}")
                }
            }
            Value::Some(inner) if d < 2 => format!("Option<{}>", go(inner, d + 1)),
            Value::Ok(inner) if d < 2 => format!("Result, Ok<{}>", go(inner, d + 1)),
            Value::Err(inner) if d < 2 => format!("Result, Err<{}>", go(inner, d + 1)),
            Value::None => "Option, None".to_string(),
            // Scalars and everything else: the type name alone, which for an
            // Int/Float/Bool is exactly the no-value rule.
            other => other.type_name(),
        }
    }
    go(v, 0)
}

/// Render a value as an Axon **literal**, or explain why it cannot be.
///
/// `AXON_FOR_RLM.md` §5, the values-persisting session. A session restores a
/// prior cell's bindings by emitting `let name = <literal>` — a literal, never
/// the original expression, because re-evaluating `let rows = expensive()`
/// would re-run `expensive()` on every subsequent cell. That side-effect replay
/// is the failure the declarations-only spike avoided by having no values at
/// all, and it is the one this must not reintroduce.
///
/// `Err(reason)` is not a failure path: an open channel or a closure is a
/// perfectly normal binding that simply cannot be written down. The caller
/// reports it as `skipped`, which is what `Engine::Snapshot` models and what
/// CPython's `dill` does.
pub fn value_as_literal(v: &Value) -> std::result::Result<String, String> {
    match v {
        Value::Int(n) => Ok(n.to_string()),
        Value::SizedInt { val, .. } => Ok(val.to_string()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Float(f) => {
            // `1` would re-parse as an i64 and silently change the binding's type.
            if f.fract() == 0.0 && f.is_finite() {
                Ok(format!("{f:.1}"))
            } else if f.is_finite() {
                Ok(format!("{f:?}"))
            } else {
                Err("non-finite float has no literal form".to_string())
            }
        }
        Value::Str(st) => {
            // Braces MUST be doubled. Axon string literals interpolate, so a
            // value containing `{` dumped verbatim produces a literal the lexer
            // rejects (`unclosed \u{7b} in interpolated string`) — and since the
            // dump becomes the next cell's prelude, that bricks the session
            // permanently: even `let n = 1` is then refused. A model building a
            // JSON-ish string hits this immediately. `{{` → `{` is the parser's
            // own convention (`parser.rs:96-101`); `}}` is doubled with it so
            // the escaping is the parser's exact inverse rather than nearly so.
            let esc = st
                .replace('\\', "\\\\")
                .replace('"', "\\\"")
                .replace('\n', "\\n")
                .replace('\t', "\\t")
                .replace('{', "{{")
                .replace('}', "}}");
            Ok(format!("\"{esc}\""))
        }
        Value::Array(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items.vals().iter() {
                out.push(value_as_literal(it)?);
            }
            Ok(format!("[{}]", out.join(", ")))
        }
        // A record. Needed because the very first binding in a realistic session
        // is a list of records — `rows` in `stateful.rs`'s chain fixture — and
        // skipping structs means the session cannot carry its own headline case.
        // Field order is sorted so the emitted literal is deterministic; a
        // HashMap's iteration order would make the session file differ run to run.
        Value::Struct(s) => {
            let mut parts = Vec::with_capacity(s.fields.len());
            for (k, v) in s.fields.by_name() {
                parts.push(format!("{k}: {}", value_as_literal(v)?));
            }
            Ok(format!("{} {{ {} }}", sym_name(s.name), parts.join(", ")))
        }
        // Option / Result / Tuple / Enum all HAVE literal syntax, and a realistic
        // session binds them constantly — `let o = parse_int(s)` is a `Result`.
        // Leaving them in the catch-all meant the most ordinary binding a model
        // writes could not cross a cell boundary.
        Value::Some(inner) => Ok(format!("Some({})", value_as_literal(inner)?)),
        Value::None => Ok("None".to_string()),
        Value::Ok(inner) => Ok(format!("Ok({})", value_as_literal(inner)?)),
        Value::Err(inner) => Ok(format!("Err({})", value_as_literal(inner)?)),
        Value::Tuple(items) => {
            let mut out = Vec::with_capacity(items.len());
            for it in items.vals().iter() {
                out.push(value_as_literal(it)?);
            }
            // A 1-tuple needs the trailing comma or it re-parses as a
            // parenthesised expression and silently changes type.
            if out.len() == 1 {
                Ok(format!("({},)", out[0]))
            } else {
                Ok(format!("({})", out.join(", ")))
            }
        }
        Value::Enum(e) => {
            let (enum_name, variant) = (sym_name(e.enum_name), sym_name(e.variant));
            if e.fields.is_empty() {
                Ok(format!("{enum_name}::{variant}"))
            } else {
                // deterministic, as for structs
                let mut parts = Vec::with_capacity(e.fields.len());
                for (k, v) in e.fields.by_name() {
                    parts.push(format!("{k}: {}", value_as_literal(v)?));
                }
                Ok(format!("{enum_name}::{variant} {{ {} }}", parts.join(", ")))
            }
        }
        // A dict. Not a *literal* — Axon has no dict literal syntax — but
        // `dict_from_pairs` is a pure total call over data already written down,
        // so it re-creates the value without re-running the computation that
        // produced it, which is the property the session actually needs.
        //
        // Two things are refused rather than fudged:
        //   * a dict whose values are not all the same shape, because
        //     `dict_from_pairs` takes `[(str, V)]` and a mixed array does not
        //     type-check — emitting it would produce a prelude that bricks the
        //     next cell;
        //   * aliasing, which is handled by the caller (see `dump_bindings`),
        //     since a single value cannot see that another binding shares it.
        Value::Dict(d) => {
            let map = d.borrow();
            if map.is_empty() {
                // `dict_from_pairs([])` has no element type to infer from.
                return Ok("dict_new()".to_string());
            }
            let tag = |v: &Value| -> &'static str {
                match v {
                    Value::Int(_) | Value::SizedInt { .. } => "int",
                    Value::Float(_) => "float",
                    Value::Bool(_) => "bool",
                    Value::Str(_) => "str",
                    Value::Array(_) => "array",
                    Value::Dict(_) => "dict",
                    Value::Struct { .. } => "struct",
                    Value::Tuple(_) => "tuple",
                    _ => "other",
                }
            };
            let first = tag(map.values().next().expect("non-empty"));
            if map.values().any(|v| tag(v) != first) {
                return Err(
                    "dict with mixed value types has no writable form (dict_from_pairs needs one \
                     element type)"
                        .to_string(),
                );
            }
            let mut parts = Vec::with_capacity(map.len());
            for (k, v) in map.iter() {
                // The key goes through the same escaping as any str.
                let kl = value_as_literal(&Value::Str(Rc::new(k.clone())))?;
                parts.push(format!("({kl}, {})", value_as_literal(v)?));
            }
            Ok(format!("dict_from_pairs([{}])", parts.join(", ")))
        }
        Value::Unit => Err("unit has no binding form".to_string()),
        // Closure/Chan/Handle/… — a session can carry a
        // value only if it can write it down, and these cannot be written as a
        // literal today. Reported by name so the caller can say WHICH binding
        // was dropped, which is the whole contract of a skip list.
        // The SHAPE, not the Rust `Debug`. This message is read by a person (or
        // a model) deciding what to do about a binding that did not survive a
        // cell; `Closure { params: ["x"], body: BinOp { op: Add, left: … } }`
        // tells them nothing actionable and buries the one word that matters.
        other => Err(format!("a {} has no literal form", value_shape(other))),
    }
}

pub fn run_program_with_discharged(
    program: &Program,
    discharged: crate::verify::Discharged,
) -> i32 {
    on_deep_stack(|| run_program_inner(program, discharged, true))
}

/// Phase 11: call a specific named function (no args) and return an exit code.
///
/// - Returns `None` if the function does not exist in the program (gate is skipped).
/// - Returns `Some(0)` if the function returns `true`, `0`, or `()` without panicking.
/// - Returns `Some(1)` if the function returns `false` or a non-zero `i64`.
/// - Returns `Some(exit_code)` for any runtime error (panic, verify, etc.).
///
/// Globals are initialized before calling so the function can read module-level lets.
pub fn run_named_fn_as_bool(program: &Program, fn_name: &str) -> Option<i32> {
    run_named_fn_as_bool_with_score(program, fn_name, None)
}

/// As [`run_named_fn_as_bool`], but supplies `score` to a gate declared with one
/// parameter (AUDIT T17 / P5-01).
pub fn run_named_fn_as_bool_with_score(
    program: &Program,
    fn_name: &str,
    score: Option<i64>,
) -> Option<i32> {
    on_deep_stack(|| {
        let mut interp = match Interp::build(program) {
            Ok(ix) => ix.with_discharged(crate::verify::Discharged::default()),
            Err(refused) => {
                eprintln!("error: {refused}");
                return Some(2);
            }
        };
        if !interp.fns.contains_key(fn_name) {
            return None;
        }
        // Initialize globals so the gate function can read module-level lets.
        if let Err(e) = interp.init_globals() {
            let code = match e {
                Flow::Exit(c) => c,
                Flow::Panic(_) => 101,
                _ => 101,
            };
            return Some(code);
        }
        let f: &FnDef = *interp.fns.get(fn_name)?;
        // AUDIT T17 (finding P5-01): this called every gate with `vec![]` and no
        // arity check, so a gate declared `fn assert_deployable(score: i64)`
        // panicked with "expected 1 args, got 0". `axon intent compile` GENERATES
        // exactly that signature, so the tool's own generator emitted a gate ABI
        // the tool's own runner could not call — and CLAUDE.md's Acid Test 2
        // failed 100% of the time on the shipped example.
        //
        // Both shapes are now accepted: a nullary gate is called as before, and a
        // 1-arg gate receives the score it is written to judge (0 when no score
        // is available, which is the conservative input for a `score >= N` gate).
        // Any other arity is a clear error rather than a panic from deep inside
        // the interpreter.
        let args: Vec<Value> = match f.params.len() {
            0 => vec![],
            1 => vec![Value::Int(score.unwrap_or(0))],
            n => {
                eprintln!(
                    "axon: gate `{fn_name}` takes {n} arguments; a deploy gate must take \
                     0 (no input) or 1 (the score to judge)"
                );
                return Some(2);
            }
        };
        let result = interp.call_fn(f, args);
        Some(match result {
            Ok(Value::Bool(true)) => 0,
            Ok(Value::Bool(false)) => 1,
            Ok(Value::Int(0)) => 0,
            Ok(Value::Int(_)) => 1,
            // A GATE THAT PRODUCED NO READABLE VERDICT IS NOT A GATE THAT
            // PASSED.
            //
            // This arm used to be `=> 0`, making Unit, Str, Struct, Option and
            // Result indistinguishable from `Bool(true)`. Measured on a
            // Critical-risk program whose simulate, stress and
            // assert_deployable all returned `Err("... FAILED")`: the deploy
            // reported `status:"deployed"`, exit 0, with all four gates listed
            // in `stages_run` — which makes it look MORE audited than the
            // skipped-gate case that `gates_skipped` exists to expose. The
            // gate was not absent; it was unreadable, and unreadable meant
            // pass.
            //
            // Two shapes reach this and neither is exotic: a gate written with
            // no return type at all, and `-> Result<bool, str>` returning
            // `Err`, which is what "no null, no exceptions — Result<T,E>
            // everywhere" pushes an author toward.
            //
            // 2, matching the arity error above: the gate's ABI is unusable,
            // which is a different fact from the gate having judged and
            // refused (1). Any non-zero code blocks the deploy.
            Ok(other) => {
                let _ = std::io::stdout().flush();
                eprintln!(
                    "axon: gate `{fn_name}` returned {} — a gate must return \
                     bool (true = pass) or i64 (0 = pass). No verdict was \
                     produced, so this is NOT a pass.",
                    other.type_name()
                );
                2
            }
            Err(Flow::Exit(c)) => c,
            Err(Flow::VerifyFailed(msg)) => {
                let _ = std::io::stdout().flush();
                eprintln!("axon: verify failed in {fn_name}: {msg}");
                VERIFY_FAILED_EXIT_CODE
            }
            Err(Flow::Halted(msg)) => {
                let _ = std::io::stdout().flush();
                eprintln!("axon: halted in {fn_name}: {msg}");
                HALTED_EXIT_CODE
            }
            Err(Flow::Panic(msg)) => {
                let _ = std::io::stdout().flush();
                eprintln!("axon: panic in {fn_name}: {msg}");
                101
            }
            Err(_) => 101,
        })
    })
}

// ── R15 resume runtime ──────────────────────────────────────────────────────────
//
// `host_await(req)` suspends the program, yields `req` to the host, and resumes
// with the host's reply. The interpreter (`Interp`/`Value`) is `!Send` (`Rc`), so
// it CANNOT cross threads — but the program can run on a worker thread that owns
// its OWN interp (created there, never moved). The worker BLOCKING on the reply
// channel is the suspension; the host (this caller's thread) regains control,
// services the request, and unblocks the worker. No `Flow` plumbing, no `unsafe`,
// no dependency — additive.
//
// SLICE 2 (this revision): the channel now carries an OWNED `Send` deep-clone of
// the payload (`SendValue`), not just `String`, so **arbitrary `Value` payloads —
// dict, struct, enum, nested arrays/options/results, tuples, closures — survive a
// suspend** on the native worker-thread substrate. This is the spec's
// safe-alternative path (governance/specs/R15-resume-runtime.md §11 Slice (2)):
// deep-clone Values to a `Send` owned form across the thread, with NO `unsafe` and
// NO new dependency — the worker-thread substrate stays. `Chan` (and any value
// transitively containing one) is identity-shared mutable state; deep-cloning it
// would silently break the sharing across the suspend, so it is REFUSED with a
// clear runtime error rather than mis-copied (the spec's "identity-shared payloads
// may be out of scope — refuse them" boundary). `Closure` IS deep-clonable (its
// `Expr` body + captures are `!Send`-free), so a fn-valued payload crosses fine.
//
// wasm has no threads, so the wasm `host_await_yield` variants read stdin / call a
// JS import on the single stack (synchronous, str-only) — those bindings serialize
// the request to its `str` form (a non-str payload there is refused).

/// An owned, `Send` deep-clone of a [`Value`], used to cross the worker-thread
/// channel that drives `host_await`. Mirrors `Value` minus the identity-shared
/// `Chan` variant: a `Chan` cannot cross (it would lose its sharing) and is refused
/// at the boundary; a `Dict` crosses as an OWNED snapshot (`Vec<(key, SendValue)>`)
/// — sound because the program retains its own `Rc` to the original dict, so what
/// the host receives is a copy to inspect, and a reply dict is freshly constructed
/// with no prior sharing to preserve.
#[derive(Debug, Clone)]
pub enum SendValue {
    Int(i64),
    SizedInt {
        val: i64,
        ty: crate::types::Type,
    },
    Float(f64),
    Decimal(i128),
    Bool(bool),
    Str(String),
    Unit,
    Array(Vec<SendValue>),
    Struct {
        name: String,
        fields: Vec<(String, SendValue)>,
    },
    Enum {
        enum_name: String,
        variant: String,
        fields: Vec<(String, SendValue)>,
    },
    Some(Box<SendValue>),
    None,
    Ok(Box<SendValue>),
    Err(Box<SendValue>),
    Closure {
        params: Vec<String>,
        body: Box<Expr>,
        captured: Vec<(String, SendValue)>,
    },
    Tuple(Vec<SendValue>),
    /// An owned snapshot of a `Dict`'s entries (sorted key order — the source is a
    /// `BTreeMap`). Reconstructs into a fresh `Rc<RefCell<…>>` dict.
    Dict(Vec<(String, SendValue)>),
}

/// Why a `Value` could not be deep-cloned to a `SendValue` for crossing the
/// suspend boundary. Today the only case is a `Chan` (identity-shared mutable
/// state) appearing in the payload.
#[derive(Debug)]
pub struct UnsendablePayload {
    /// A human-readable path to the offending value (e.g. `.q` / `[2]`).
    pub path: String,
}

impl SendValue {
    /// Deep-clone a `Value` into an owned `Send` form. Returns `Err` if the value
    /// (transitively) contains a `Chan` — identity-shared mutable state that cannot
    /// cross a suspend without silently losing its sharing.
    pub fn from_value(v: &Value) -> Result<SendValue, UnsendablePayload> {
        Self::from_value_at(v, String::new())
    }

    /// The request `host_await_val`/`host_await_val_opt` hands the host:
    /// `Ok(None)` when the run's substrate refuses `v` unsent (the builtin
    /// then reports no host driver). A payload holding a `Chan` or a handle
    /// is refused first, wherever it sits, with its path.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn request(v: &Value) -> Result<Option<SendValue>, UnsendablePayload> {
        Self::from_value(v).map(Some)
    }

    /// AX-61: the wasm32 substrates carry only a `Str` (see
    /// [`SendValue::as_str_payload`]), so any other payload is refused
    /// without the deep copy `from_value` would make: copying a closure
    /// clones its body once per level of source nesting, and the payload
    /// once per level of data nesting, outside the stack budget. The `Chan`
    /// check runs on an explicit stack in `from_value_at`'s order, so the
    /// path it reports is the same.
    #[cfg(target_arch = "wasm32")]
    pub fn request(v: &Value) -> Result<Option<SendValue>, UnsendablePayload> {
        if let Value::Str(s) = v {
            return Ok(Some(SendValue::Str(String::clone(s))));
        }
        let mut stack: Vec<(Value, String)> = vec![(v.clone(), String::new())];
        let mut kids: Vec<(Value, String)> = Vec::new();
        while let Some((v, path)) = stack.pop() {
            match &v {
                Value::Chan(_) | Value::Handle { .. } => {
                    let path = if path.is_empty() {
                        "<root>".to_string()
                    } else {
                        path
                    };
                    return Err(UnsendablePayload { path });
                }
                Value::Array(xs) | Value::Tuple(xs) => kids.extend(
                    xs.iter()
                        .enumerate()
                        .map(|(i, x)| (x.clone(), format!("{path}[{i}]"))),
                ),
                Value::Struct(s) => kids.extend(
                    s.fields
                        .by_name()
                        .into_iter()
                        .map(|(k, x)| (x.clone(), format!("{path}.{k}"))),
                ),
                Value::Enum(e) => kids.extend(
                    e.fields
                        .by_name()
                        .into_iter()
                        .map(|(k, x)| (x.clone(), format!("{path}.{k}"))),
                ),
                Value::Some(b) => kids.push(((**b).clone(), format!("{path}.Some"))),
                Value::Ok(b) => kids.push(((**b).clone(), format!("{path}.Ok"))),
                Value::Err(b) => kids.push(((**b).clone(), format!("{path}.Err"))),
                Value::Closure(c) => {
                    let snapshot = c.captured.borrow();
                    let mut named: Vec<(Rc<str>, &Value)> =
                        snapshot.iter().map(|(k, x)| (sym_name(*k), x)).collect();
                    named.sort_by(|a, b| a.0.cmp(&b.0));
                    kids.extend(
                        named
                            .into_iter()
                            .map(|(k, x)| (x.clone(), format!("{path}.capture[{k}]"))),
                    );
                }
                Value::Dict(d) => kids.extend(
                    d.borrow()
                        .iter()
                        .map(|(k, x)| (x.clone(), format!("{path}.{k}"))),
                ),
                _ => {}
            }
            stack.extend(kids.drain(..).rev());
        }
        Ok(None)
    }

    fn from_value_at(v: &Value, path: String) -> Result<SendValue, UnsendablePayload> {
        fn arr(xs: &[Value], path: &str) -> Result<Vec<SendValue>, UnsendablePayload> {
            xs.iter()
                .enumerate()
                .map(|(i, x)| SendValue::from_value_at(x, format!("{path}[{i}]")))
                .collect()
        }
        fn fields(m: &Fields, path: &str) -> Result<Vec<(String, SendValue)>, UnsendablePayload> {
            // Sorted by name for a deterministic owned snapshot (the parity
            // round-trip must be stable).
            m.by_name()
                .into_iter()
                .map(|(k, v)| {
                    Ok((
                        k.to_string(),
                        SendValue::from_value_at(v, format!("{path}.{k}"))?,
                    ))
                })
                .collect()
        }
        Ok(match v {
            Value::Int(n) => SendValue::Int(*n),
            Value::SizedInt { val, ty } => SendValue::SizedInt {
                val: *val,
                ty: ty.to_type(),
            },
            Value::Float(f) => SendValue::Float(*f),
            Value::Decimal(m) => SendValue::Decimal(**m),
            Value::Bool(b) => SendValue::Bool(*b),
            Value::Str(s) => SendValue::Str(String::clone(s)),
            Value::Unit => SendValue::Unit,
            Value::Array(xs) => SendValue::Array(arr(&xs.vals(), &path)?),
            Value::Struct(s) => SendValue::Struct {
                name: sym_name(s.name).to_string(),
                fields: fields(&s.fields, &path)?,
            },
            Value::Enum(e) => SendValue::Enum {
                enum_name: sym_name(e.enum_name).to_string(),
                variant: sym_name(e.variant).to_string(),
                fields: fields(&e.fields, &path)?,
            },
            Value::Some(b) => {
                SendValue::Some(Box::new(Self::from_value_at(b, format!("{path}.Some"))?))
            }
            Value::None => SendValue::None,
            Value::Ok(b) => SendValue::Ok(Box::new(Self::from_value_at(b, format!("{path}.Ok"))?)),
            Value::Err(b) => {
                SendValue::Err(Box::new(Self::from_value_at(b, format!("{path}.Err"))?))
            }
            Value::Closure(c) => {
                let (code, captured) = (&c.code, &c.captured);
                let snapshot = captured.borrow();
                let mut named: Vec<(Rc<str>, &Value)> =
                    snapshot.iter().map(|(k, v)| (sym_name(*k), v)).collect();
                named.sort_by(|a, b| a.0.cmp(&b.0));
                let cap: Result<Vec<_>, _> = named
                    .into_iter()
                    .map(|(k, v)| {
                        Ok((
                            k.to_string(),
                            Self::from_value_at(v, format!("{path}.capture[{k}]"))?,
                        ))
                    })
                    .collect();
                SendValue::Closure {
                    params: code
                        .params
                        .iter()
                        .map(|p| sym_name(*p).to_string())
                        .collect(),
                    body: Box::new(code.body.clone()),
                    captured: cap?,
                }
            }
            Value::Tuple(xs) => SendValue::Tuple(arr(&xs.vals(), &path)?),
            Value::Dict(d) => {
                let borrowed = d.borrow();
                let mut out = Vec::with_capacity(borrowed.len());
                for (k, val) in borrowed.iter() {
                    out.push((k.clone(), Self::from_value_at(val, format!("{path}.{k}"))?));
                }
                SendValue::Dict(out)
            }
            Value::Chan(_) => {
                return Err(UnsendablePayload {
                    path: if path.is_empty() {
                        "<root>".to_string()
                    } else {
                        path
                    },
                });
            }
            // R13: a native handle is identity-bound to the in-process handle
            // table (like a Chan) — it cannot cross a suspend boundary. Refuse
            // rather than corrupt (the table index would be meaningless on the
            // other side).
            Value::Handle { .. } => {
                return Err(UnsendablePayload {
                    path: if path.is_empty() {
                        "<root>".to_string()
                    } else {
                        path
                    },
                });
            }
        })
    }

    /// Reconstruct a `Value` from this owned `Send` form, allocating fresh
    /// `Rc<RefCell<…>>` cells for any `Dict` (there is no prior sharing to preserve
    /// — a reply value is freshly built by the host).
    pub fn into_value(self) -> Value {
        match self {
            SendValue::Int(n) => Value::Int(n),
            SendValue::SizedInt { val, ty } => match IntWidth::of_type(&ty) {
                Some(ty) => Value::SizedInt { val, ty },
                None => Value::Int(val),
            },
            SendValue::Float(f) => Value::Float(f),
            SendValue::Decimal(m) => Value::decimal(m),
            SendValue::Bool(b) => Value::Bool(b),
            SendValue::Str(s) => Value::Str(Rc::new(s)),
            SendValue::Unit => Value::Unit,
            SendValue::Array(xs) => {
                Value::Array(Rc::new(xs.into_iter().map(Self::into_value).collect()))
            }
            SendValue::Struct { name, fields } => Value::record(
                intern(&name),
                Fields::from_pairs(
                    fields
                        .into_iter()
                        .map(|(k, v)| (intern(&k), v.into_value())),
                ),
            ),
            SendValue::Enum {
                enum_name,
                variant,
                fields,
            } => Value::Enum(Rc::new(EnumVal {
                enum_name: intern(&enum_name),
                variant: intern(&variant),
                fields: Fields::from_pairs(
                    fields
                        .into_iter()
                        .map(|(k, v)| (intern(&k), v.into_value())),
                ),
            })),
            SendValue::Some(b) => Value::Some(VBox::new(b.into_value())),
            SendValue::None => Value::None,
            SendValue::Ok(b) => Value::Ok(VBox::new(b.into_value())),
            SendValue::Err(b) => Value::Err(VBox::new(b.into_value())),
            SendValue::Closure {
                params,
                body,
                captured,
            } => Value::Closure(Rc::new(ClosureVal::plain(
                Rc::new(ClosureCode::unresolved(
                    params.iter().map(|p| intern(p)).collect(),
                    *body,
                )),
                // A closure that crossed the host boundary gets a FRESH capture
                // cell: the SendValue path is a deep clone by construction (a
                // shared cell is exactly what it cannot carry), so the two sides
                // are independent counters, not aliases (T40 + R15 Slice 2). A
                // `SendValue` carries no taint, so the cell has none: a sealed
                // frame that calls it stops (`Captures::plain`; R51 S3's deep
                // copy edge is where a sealed crossing gets its taints).
                Captures::plain(
                    captured
                        .into_iter()
                        .map(|(k, v)| (intern(&k), v.into_value()))
                        .collect(),
                ),
            ))),
            SendValue::Tuple(xs) => Value::tuple(xs.into_iter().map(Self::into_value).collect()),
            SendValue::Dict(entries) => {
                let map: std::collections::BTreeMap<String, Value> = entries
                    .into_iter()
                    .map(|(k, v)| (k, v.into_value()))
                    .collect();
                Value::Dict(Rc::new(RefCell::new(map.into())))
            }
        }
    }

    /// The `str` projection of a payload, for the str-only wasm bindings (stdin /
    /// JS import): a `Str` is itself; anything else is `None` (those substrates only
    /// model a text reply channel, so a non-str request there is refused).
    #[cfg(target_arch = "wasm32")]
    fn as_str_payload(&self) -> Option<&str> {
        match self {
            SendValue::Str(s) => Some(s),
            _ => None,
        }
    }
}

/// The str-form `host_await` reply contract: EOF (`None`) → `""`; a `Str` reply →
/// itself; any other reply Value (a Value-host answering the str form) → its display
/// rendering. Used by the str-typed `host_await`/`host_await_opt` builtins.
pub(crate) fn send_reply_to_string(reply: Option<SendValue>) -> String {
    match reply {
        None => String::new(),
        Some(SendValue::Str(s)) => s,
        Some(other) => send_value_display(&other),
    }
}

/// A best-effort text rendering of a `SendValue` (shared by the str reply contract
/// and the str-host wrapper). Scalars render naturally; composites render a compact
/// form.
pub(crate) fn send_value_display(v: &SendValue) -> String {
    match v {
        SendValue::Int(n) => n.to_string(),
        SendValue::SizedInt { val, .. } => val.to_string(),
        SendValue::Float(f) => f.to_string(),
        SendValue::Decimal(m) => crate::decimal::format_decimal(*m),
        SendValue::Bool(b) => b.to_string(),
        SendValue::Str(s) => s.clone(),
        SendValue::Unit => "()".to_string(),
        SendValue::Array(xs) | SendValue::Tuple(xs) => {
            let inner: Vec<String> = xs.iter().map(send_value_display).collect();
            format!("[{}]", inner.join(", "))
        }
        SendValue::Struct { name, fields } => {
            let inner: Vec<String> = fields
                .iter()
                .map(|(k, v)| format!("{k}: {}", send_value_display(v)))
                .collect();
            format!("{name} {{ {} }}", inner.join(", "))
        }
        SendValue::Enum {
            enum_name, variant, ..
        } => format!("{enum_name}::{variant}"),
        SendValue::Some(b) => format!("Some({})", send_value_display(b)),
        SendValue::None => "None".to_string(),
        SendValue::Ok(b) => format!("Ok({})", send_value_display(b)),
        SendValue::Err(b) => format!("Err({})", send_value_display(b)),
        SendValue::Closure { .. } => "<fn>".to_string(),
        SendValue::Dict(entries) => {
            let inner: Vec<String> = entries
                .iter()
                .map(|(k, v)| format!("{k}: {}", send_value_display(v)))
                .collect();
            format!("{{ {} }}", inner.join(", "))
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
struct HostChannels {
    req_tx: std::sync::mpsc::Sender<SendValue>,
    // `None` reply = end-of-input (host has no more to give) — distinct from an
    // empty-string reply (a blank line). `host_await` collapses both to ""; the
    // EOF-aware `host_await_opt` surfaces the distinction as `None`.
    rep_rx: std::sync::mpsc::Receiver<Option<SendValue>>,
}
#[cfg(not(target_arch = "wasm32"))]
thread_local! {
    static HOST_AWAIT: RefCell<Option<HostChannels>> = const { RefCell::new(None) };
}

// ── vsock host_await substrate (Linux, axon-vm microVM) ───────────────────────
//
// When AXON_VM_VSOCK_PORT is set the interpreter uses AF_VSOCK to communicate
// with the axon-vm launcher (CID 2 = host) instead of the worker-thread channel.
// The interpreter runs single-threaded; host_await_yield opens a connection per
// suspension (cheap — vsock connect is ~10µs in a VM).
//
// Protocol: length-prefixed JSON over a stream socket.
//   send: 4-byte LE u32 length, then `length` UTF-8 bytes (the request payload)
//   recv: 4-byte LE u32 length, then `length` bytes (the reply); length=0 → EOF

#[cfg(target_os = "linux")]
thread_local! {
    // File descriptor of the open vsock connection, or -1 when not in vsock mode.
    static VSOCK_PORT: std::cell::Cell<i32> = const { std::cell::Cell::new(-1) };
}

#[cfg(target_os = "linux")]
fn vsock_send_recv(port: u32, req: &str) -> Result<Option<String>, ()> {
    use std::io::{Read, Write};
    use std::os::unix::io::FromRawFd;

    // AF_VSOCK = 40, VMADDR_CID_HOST = 2
    const AF_VSOCK: libc::c_int = 40;
    const VMADDR_CID_HOST: u32 = 2;

    // sockaddr_vm layout: family(u16), reserved(u16), port(u32), cid(u32), flags(u8), pad(3)
    #[repr(C)]
    struct SockaddrVm {
        svm_family: u16,
        svm_reserved1: u16,
        svm_port: u32,
        svm_cid: u32,
        svm_flags: u8,
        svm_zero: [u8; 3],
    }

    let fd = unsafe { libc::socket(AF_VSOCK, libc::SOCK_STREAM | libc::SOCK_CLOEXEC, 0) };
    if fd < 0 {
        return Err(());
    }

    let addr = SockaddrVm {
        svm_family: AF_VSOCK as u16,
        svm_reserved1: 0,
        svm_port: port,
        svm_cid: VMADDR_CID_HOST,
        svm_flags: 0,
        svm_zero: [0; 3],
    };

    let r = unsafe {
        libc::connect(
            fd,
            &addr as *const SockaddrVm as *const libc::sockaddr,
            std::mem::size_of::<SockaddrVm>() as libc::socklen_t,
        )
    };
    if r < 0 {
        unsafe {
            libc::close(fd);
        }
        return Err(());
    }

    // Safety: we just created fd and it's a valid stream socket.
    let mut stream = unsafe { std::net::TcpStream::from_raw_fd(fd) };

    // Send: 4-byte LE length + payload bytes.
    let payload = req.as_bytes();
    let len_bytes = (payload.len() as u32).to_le_bytes();
    stream.write_all(&len_bytes).map_err(|_| ())?;
    stream.write_all(payload).map_err(|_| ())?;
    stream.flush().map_err(|_| ())?;

    // Recv: 4-byte LE length + reply bytes.
    let mut lbuf = [0u8; 4];
    stream.read_exact(&mut lbuf).map_err(|_| ())?;
    let reply_len = u32::from_le_bytes(lbuf) as usize;
    if reply_len == 0 {
        return Ok(None); // EOF / end-of-input sentinel
    }
    let mut rbuf = vec![0u8; reply_len];
    stream.read_exact(&mut rbuf).map_err(|_| ())?;
    Ok(Some(String::from_utf8_lossy(&rbuf).into_owned()))
}

/// Run `program` using vsock for host_await (axon-vm microVM substrate).
/// The host (axon-vm launcher) listens on `vsock_port`; we connect per suspension.
#[cfg(target_os = "linux")]
pub fn run_suspendable_vsock(program: &Program, vsock_port: u32) -> i32 {
    VSOCK_PORT.with(|c| c.set(vsock_port as i32));
    let code =
        on_deep_stack(|| run_program_inner(program, crate::verify::Discharged::default(), true));
    VSOCK_PORT.with(|c| c.set(-1));
    code
}

// ── Unix-socket / hypercall host_await substrate (axon-guest-kernel bridge) ───
//
// When AXON_HOST_SOCKET is set the interpreter uses a Unix domain socket to
// communicate with the axon-guest-kernel's hypercall bridge instead of vsock or
// stdin/stdout. The guest kernel creates the socket before exec-ing the
// interpreter and issues the actual VMCALL to the host on our behalf.
//
// Protocol: same 4-byte-LE-length-prefixed UTF-8 as the vsock substrate.
//   send: 4-byte LE u32 length, then `length` UTF-8 bytes (the request payload)
//   recv: 4-byte LE u32 length, then `length` bytes (the reply); length=0 → EOF
//
// The socket is opened fresh per host_await call (cheap on Unix) so the
// interpreter remains single-threaded with no persistent fd lifecycle.

#[cfg(unix)]
thread_local! {
    // Active Unix socket path set by run_suspendable_hypercall; None when inactive.
    static UNIX_SOCK_PATH: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Open a connection to the Unix stream socket at `path`, perform one
/// length-prefixed request/reply round-trip, and close the socket.
/// Returns `Ok(Some(reply))` on success, `Ok(None)` when the peer sends
/// length=0 (end-of-input sentinel), or `Err(())` on any I/O failure.
/// Retries once with a 20 ms pause if the first `connect` fails — the
/// guest-kernel may be fractionally behind when exec-ing the interpreter.
#[cfg(unix)]
fn unix_socket_roundtrip(path: &str, req: &str) -> Result<Option<String>, ()> {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    let mut stream = UnixStream::connect(path)
        .or_else(|_| {
            std::thread::sleep(std::time::Duration::from_millis(20));
            UnixStream::connect(path)
        })
        .map_err(|_| ())?;

    // Send: 4-byte LE length + payload bytes.
    let payload = req.as_bytes();
    stream
        .write_all(&(payload.len() as u32).to_le_bytes())
        .map_err(|_| ())?;
    stream.write_all(payload).map_err(|_| ())?;
    stream.flush().map_err(|_| ())?;

    // Recv: 4-byte LE length + reply bytes.
    let mut lbuf = [0u8; 4];
    stream.read_exact(&mut lbuf).map_err(|_| ())?;
    let reply_len = u32::from_le_bytes(lbuf) as usize;
    if reply_len == 0 {
        return Ok(None); // EOF / end-of-input sentinel
    }
    let mut rbuf = vec![0u8; reply_len];
    stream.read_exact(&mut rbuf).map_err(|_| ())?;
    Ok(Some(String::from_utf8_lossy(&rbuf).into_owned()))
}

/// Run `program` using the axon-guest-kernel hypercall substrate for `host_await`.
///
/// The guest kernel exposes a Unix stream socket at the path given by
/// `AXON_HOST_SOCKET` (default: `/tmp/axon-host.sock`) that forwards each
/// `host_await` request to the host via VMCALL and returns the host's reply.
/// The interpreter itself runs entirely in ring-3 userspace — it never issues
/// VMCALL directly.
///
/// On connection failure (socket absent, wrong path) `unix_socket_roundtrip`
/// returns `Err(())`, which `host_await_yield` surfaces as end-of-input
/// (`None`) rather than panicking. A program that never calls `host_await`
/// completes normally even when no socket exists at the default path.
#[cfg(unix)]
pub fn run_suspendable_hypercall(program: &Program) -> i32 {
    let sock_path =
        std::env::var("AXON_HOST_SOCKET").unwrap_or_else(|_| "/tmp/axon-host.sock".to_string());
    UNIX_SOCK_PATH.with(|c| *c.borrow_mut() = Some(sock_path));
    let code =
        on_deep_stack(|| run_program_inner(program, crate::verify::Discharged::default(), true));
    UNIX_SOCK_PATH.with(|c| *c.borrow_mut() = None);
    code
}

/// Reach the active host channels from inside `host_await` (interp/builtins.rs).
/// Returns `Ok(Some(reply))`, `Ok(None)` at end-of-input, or `Err(())` if there is
/// no host at all (a bare `axon run`). NATIVE: the `SendValue` request (an owned
/// deep-clone of the program's payload — see the builtin dispatch) crosses the
/// worker→host channel; the worker BLOCKS on the reply channel (that IS the
/// suspension); the host's `SendValue` reply crosses back and is reconstructed into
/// a `Value` by the caller.
#[cfg(not(target_arch = "wasm32"))]
pub(crate) fn host_await_yield(req: SendValue) -> Result<Option<SendValue>, ()> {
    // Unix-socket hypercall path: takes priority over vsock and the worker-thread
    // channel. Active when run_suspendable_hypercall set UNIX_SOCK_PATH.
    #[cfg(unix)]
    {
        let path = UNIX_SOCK_PATH.with(|c| c.borrow().clone());
        if let Some(path) = path {
            let req_str = match &req {
                SendValue::Str(s) => s.clone(),
                other => format!("{other:?}"),
            };
            return unix_socket_roundtrip(&path, &req_str).map(|opt| opt.map(SendValue::Str));
        }
    }

    // vsock path: if running inside axon-vm, bypass the worker-thread channel.
    #[cfg(target_os = "linux")]
    {
        let port = VSOCK_PORT.with(|c| c.get());
        if port >= 0 {
            let req_str = match &req {
                SendValue::Str(s) => s.clone(),
                other => format!("{:?}", other),
            };
            return vsock_send_recv(port as u32, &req_str).map(|opt| opt.map(SendValue::Str));
        }
    }

    HOST_AWAIT.with(|h| {
        let guard = h.borrow();
        match &*guard {
            Some(ch) => {
                ch.req_tx.send(req).map_err(|_| ())?;
                // Holding the borrow across this blocking recv is fine: nothing
                // else on THIS thread touches HOST_AWAIT while the worker is parked.
                ch.rep_rx.recv().map_err(|_| ())
            }
            None => Err(()),
        }
    })
}

/// WASI wasm (wasmtime): no threads, so there's no worker/channel substrate. Read
/// the reply from stdin DIRECTLY on the single stack — a synchronous host_await
/// that works under `wasmtime` (wasip1) with piped stdin, the same observable
/// behavior as native's stdio host. Writes the request (a prompt) to stdout,
/// reads one line as the reply (trailing newline stripped; EOF → `None`). This
/// makes interactive Axon programs run on headless wasm.
#[cfg(all(target_arch = "wasm32", target_os = "wasi"))]
pub(crate) fn host_await_yield(req: SendValue) -> Result<Option<SendValue>, ()> {
    use std::io::{BufRead, Write};
    // str-only substrate: a non-str payload is refused (no value channel here).
    let req = req.as_str_payload().ok_or(())?;
    print!("{req}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    match std::io::stdin().lock().read_line(&mut line) {
        Ok(0) | Err(_) => Ok(None), // EOF / no stdin → end-of-input
        Ok(_) => Ok(Some(SendValue::Str(
            line.trim_end_matches(['\n', '\r']).to_string(),
        ))),
    }
}

/// BROWSER wasm (`wasm32-unknown-unknown`): no stdin, no threads. Yield the request
/// to JavaScript via an imported `axon_host_await(req_ptr, req_len, out_ptr,
/// out_cap) -> i64` host function: JS reads the request from linear memory, writes
/// up to `out_cap` reply bytes into `out_ptr`, and returns the reply byte length
/// (or a negative value for end-of-input → `None`). v1 is SYNCHRONOUS (the page
/// answers immediately — a tool-call, a pre-filled value); the ASYNC cases (input
/// box, fetch, frame loop) are the R15 §13 B3 follow-on, where `wasm-opt
/// --asyncify` lets THIS SAME import suspend the module and resume after a JS
/// Promise. The axon-wasm cdylib hosts this binding; JS must supply the import.
#[cfg(all(target_arch = "wasm32", not(target_os = "wasi")))]
pub(crate) fn host_await_yield(req: SendValue) -> Result<Option<SendValue>, ()> {
    extern "C" {
        fn axon_host_await(
            req_ptr: *const u8,
            req_len: usize,
            out_ptr: *mut u8,
            out_cap: usize,
        ) -> i64;
    }
    // str-only substrate: a non-str payload is refused (no value channel here).
    let req = req.as_str_payload().ok_or(())?;
    // A generous fixed reply buffer (the page truncates to fit). 64 KiB covers a
    // prompt reply / tool result; larger payloads should stream (future).
    let mut buf = vec![0u8; 64 * 1024];
    let n = unsafe { axon_host_await(req.as_ptr(), req.len(), buf.as_mut_ptr(), buf.len()) };
    if n < 0 {
        return Ok(None); // end-of-input
    }
    let n = (n as usize).min(buf.len());
    buf.truncate(n);
    Ok(Some(SendValue::Str(
        String::from_utf8_lossy(&buf).into_owned(),
    )))
}

/// Run `program` with a `Value`-aware HOST driving its `host_await` suspensions.
/// `host(req)` is called once per `host_await`, on THIS thread, with the owned
/// `SendValue` deep-clone of the program's request payload; its return —
/// `Some(reply)` or `None` at end-of-input — is fed back (reconstructed into a
/// `Value`) as the resume value. Returns the program's exit code. (R15 Slice 2 —
/// arbitrary `Value` payloads: dict/struct/enum/tuple/closure all cross.)
///
/// NATIVE-ONLY: the substrate is a worker thread (`std::thread::scope`), which is
/// unavailable on `wasm32` (`thread::spawn` traps). The browser binding (R7c)
/// drives `host_await` via Asyncify + a JS import instead (R15 §13), NOT this
/// thread-based path. The owned `SendValue` deep-clone is what makes a `!Send`
/// `Value` payload cross the worker channel (§11 Slice (2) safe alternative).
#[cfg(not(target_arch = "wasm32"))]
pub fn run_suspendable_values(
    program: &Program,
    host: impl FnMut(SendValue) -> Option<SendValue>,
) -> i32 {
    run_suspendable_values_inner(program, host, /*map_status=*/ true)
}

/// [`run_suspendable_values`] returning the value the program PRODUCED rather
/// than the status a caller would observe — see [`run_program_unmapped`] for why
/// the two are separate, and why reading a result out of a program must not go
/// through the process-status rule.
#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) fn run_suspendable_values_unmapped(
    program: &Program,
    host: impl FnMut(SendValue) -> Option<SendValue>,
) -> i32 {
    run_suspendable_values_inner(program, host, /*map_status=*/ false)
}

#[cfg(not(target_arch = "wasm32"))]
fn run_suspendable_values_inner(
    program: &Program,
    mut host: impl FnMut(SendValue) -> Option<SendValue>,
    map_status: bool,
) -> i32 {
    use std::sync::mpsc::channel;
    let (req_tx, req_rx) = channel::<SendValue>(); // worker → host (await requests)
    let (rep_tx, rep_rx) = channel::<Option<SendValue>>(); // host → worker (replies; None = EOF)

    // AUDIT T16 (finding INTERP-H03): this used a bare `scope.spawn`, giving the
    // worker the DEFAULT ~2 MiB stack. The tree-walking interpreter burns a lot
    // of native stack per call, so a suspendable program recursing a few hundred
    // frames deep hit a hard stack overflow (SIGSEGV) instead of the graceful
    // RECURSION_LIMIT panic — the guard exists precisely so the limit trips
    // first. The two sibling substrates (vsock, hypercall) already size their
    // threads this way, so this was an isolated omission rather than a design
    // choice. Same sizing as `on_deep_stack`.
    //
    // The worker must remain the thread that installs HOST_AWAIT (thread-local),
    // so the closure is sized in place rather than nested inside another thread.
    let stack = stack_size_for_depth(resolve_max_depth());
    std::thread::scope(|scope| {
        let worker = std::thread::Builder::new()
            .stack_size(stack)
            .spawn_scoped(scope, move || {
                HOST_AWAIT.with(|h| *h.borrow_mut() = Some(HostChannels { req_tx, rep_rx }));
                let code =
                    run_program_inner(program, crate::verify::Discharged::default(), map_status);
                // Drop the channels → req_tx closes → the host loop below ends.
                HOST_AWAIT.with(|h| *h.borrow_mut() = None);
                code
            })
            .expect("spawn interpreter worker");
        // Host loop: service each suspension until the worker finishes (req_tx drops).
        while let Ok(req) = req_rx.recv() {
            let _ = rep_tx.send(host(req));
        }
        worker.join().unwrap_or(101)
    })
}

/// Run `program` with a str-only HOST driving its `host_await` suspensions — the
/// interactive-text path (a prompt loop / REPL / the stdio driver). `host(req)` is
/// called once per `host_await` with the request's `str` projection; its
/// `Option<String>` reply is fed back. A non-str request payload is rendered to its
/// `Value` display string for the prompt (text hosts can't carry structured
/// payloads); use [`run_suspendable_values`] for full `Value` fidelity. (R15.)
#[cfg(not(target_arch = "wasm32"))]
pub fn run_suspendable(program: &Program, host: impl FnMut(&str) -> Option<String>) -> i32 {
    run_suspendable_str_inner(program, host, /*map_status=*/ true)
}

/// [`run_suspendable`] returning the value the program PRODUCED — the same
/// distinction [`run_program_unmapped`] draws, for the suspendable substrate.
#[cfg(all(not(target_arch = "wasm32"), test))]
pub(crate) fn run_suspendable_unmapped(
    program: &Program,
    host: impl FnMut(&str) -> Option<String>,
) -> i32 {
    run_suspendable_str_inner(program, host, /*map_status=*/ false)
}

#[cfg(not(target_arch = "wasm32"))]
fn run_suspendable_str_inner(
    program: &Program,
    mut host: impl FnMut(&str) -> Option<String>,
    map_status: bool,
) -> i32 {
    run_suspendable_values_inner(
        program,
        |req| {
            let s = match &req {
                SendValue::Str(s) => s.clone(),
                other => send_value_display(other),
            };
            host(&s).map(SendValue::Str)
        },
        map_status,
    )
}

/// wasm32 has no OS threads, so the worker-thread host-driver substrate can't run
/// here. Run the program directly with NO host driver: a `host_await` call then
/// hits the clean "called outside a suspendable run (no host driver)" panic
/// (exit 101), rather than trapping on `thread::spawn`. The browser binding (R7c)
/// will drive `host_await` via Asyncify + a JS import (R15 §13) — a different
/// substrate that replaces this one on wasm, with the same surface + semantics.
#[cfg(target_arch = "wasm32")]
pub fn run_suspendable(program: &Program, _host: impl FnMut(&str) -> Option<String>) -> i32 {
    run_program_inner(program, crate::verify::Discharged::default(), true)
}

/// wasm `Value`-aware variant — same no-thread story as `run_suspendable`: there is
/// no worker-thread substrate here, so the program runs with no host driver and a
/// `host_await` call hits the clean "no host driver" panic. The browser binding
/// (R7c) drives `host_await` via Asyncify + a JS import (str-only, R15 §13).
#[cfg(target_arch = "wasm32")]
pub fn run_suspendable_values(
    program: &Program,
    _host: impl FnMut(SendValue) -> Option<SendValue>,
) -> i32 {
    run_program_inner(program, crate::verify::Discharged::default(), true)
}

/// The default CLI host for `host_await`: write the request (a prompt) to stdout,
/// then read a line from stdin as the reply (trailing newline stripped). EOF →
/// `None` (end-of-input), which `host_await_opt` surfaces so a read loop can stop;
/// plain `host_await` collapses it to "". This makes an interactive Axon program —
/// a prompt loop, a REPL, a quiz — work under a plain `axon run`. (R15 v0; the
/// program's own `println`s and the prompt share stdout, ordered by the protocol.)
pub fn run_suspendable_stdio(program: &Program) -> i32 {
    use std::io::{BufRead, Write};
    let stdin = std::io::stdin();
    run_suspendable(program, |prompt| {
        print!("{prompt}");
        let _ = std::io::stdout().flush();
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) | Err(_) => None, // EOF or error → end-of-input
            Ok(_) => Some(line.trim_end_matches(['\n', '\r']).to_string()),
        }
    })
}

/// AUDIT T35 (finding RT-02): tell the AI runtime which hosts this program was
/// statically permitted to reach.
///
/// `axon check` validates `ai_complete` against the IMPLICIT constant host
/// `api.anthropic.com`, but the runtime resolved `AXON_AI_BASE_URL` /
/// `ANTHROPIC_BASE_URL` at call time — so the host the checker approved and the
/// host actually dialled were independent values, bridgeable by any program with
/// an fs:write grant via a `.env` file. Reproduced end-to-end: a program whose
/// `@[contained(net: ["api.anthropic.com"])]` passed `axon check` exit 0 sent its
/// prompt and the real `x-api-key` to `127.0.0.1`.
///
/// The union of every declared net allowlist is pinned. A program that declares
/// no `@[contained]` net grant is not pinned — it never made a claim to violate.
/// The union of every `@[contained(net: [...])]` host the program declares,
/// across top-level fns and impl methods. Empty = the program made no net claim.
///
/// Always compiled (not feature-gated) so it stays unit-testable in the default
/// build, where the `asi-runtime` AI runtime it feeds is absent.
#[cfg_attr(not(feature = "asi-runtime"), allow(dead_code))]
fn declared_net_hosts(program: &Program) -> Vec<String> {
    let mut hosts: Vec<String> = Vec::new();
    let mut push = |c: &Option<crate::ast::ContainedSpec>| {
        if let Some(c) = c {
            for h in &c.net_allow {
                if !hosts.contains(h) {
                    hosts.push(h.clone());
                }
            }
        }
    };
    for item in &program.items {
        match item {
            Item::FnDef(f) => push(&f.contained),
            Item::ImplBlock(b) => {
                for m in &b.methods {
                    push(&m.contained);
                }
            }
            _ => {}
        }
    }
    if !hosts.is_empty() {
        // The implicit endpoint every AI builtin contacts is always permitted —
        // it is exactly what the static checker validated the grant against, so a
        // program that reaches an AI builtin at all has already been required to
        // declare it.
        let implicit = crate::capabilities::ai_implicit_host();
        if !hosts.iter().any(|h| h == implicit) {
            hosts.push(implicit.to_string());
        }
    }
    hosts
}

#[cfg(feature = "asi-runtime")]
fn pin_ai_net_allowlist(program: &Program) {
    let hosts = declared_net_hosts(program);
    if !hosts.is_empty() {
        axon_ai::pin_net_allowlist(hosts);
    }
}

/// Without the `asi-runtime` feature there is no live AI runtime to pin.
#[cfg(not(feature = "asi-runtime"))]
fn pin_ai_net_allowlist(_program: &Program) {}

/// `map_status` decides whether the value the program produced is turned into a
/// PROCESS EXIT STATUS (`stated_exit_status` / `returned_exit_status`) or handed
/// back raw.
///
/// The rule belongs at the process boundary and nowhere else. Two callers read
/// `main`'s return as a *value* rather than as a status, and must not see it
/// rewritten: the R10 G1 oracle, which compares `(exit_code, stdout)` between an
/// original and a transformed program — collapsing 6 and 7 both to 1 would make
/// a meaning-changing rewrite look equivalent — and the interpreter's own tests,
/// which use the return as the cheapest way to read a computed result out.
/// R44 Slice 1 — session capture, the API twin of `AXON_DUMP_BINDINGS`.
///
/// The prototype this grew from was driven entirely by env vars, which is fine
/// for an external driver but absurd for `axon session`: the session process
/// would have to set an environment variable on itself to talk to its own
/// interpreter, and then read its own answer back off the filesystem. This flag
/// is the same signal delivered in-process. The env vars still work — they are
/// the surface existing tests and external drivers already use.
static SESSION_CAPTURE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `(materialised_bindings, shape_inventory)` from the most recent session run.
///
/// Taken, not read: leaving a stale result behind would let a cell that produced
/// nothing silently inherit the previous cell's bindings.
///
/// A `Mutex`, NOT a `thread_local` — `on_deep_stack` runs the program on a
/// separate thread (the interpreter needs a bigger stack than the default), so a
/// thread-local result is stashed on a thread the caller never sees and every
/// cell reads `None`. That mistake presents exactly as "every cell failed at
/// runtime", which is a misleading enough symptom to be worth the comment.
static SESSION_RESULT: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);

/// The reason the last run aborted, for a caller that cannot see stderr.
///
/// The abort arms below print `axon: panic: …` and return an exit code, and that
/// was the whole of the reporting. `axon session --protocol jsonl` builds its
/// frame from the exit code, so a cell that panicked came back as
/// `{"ok":false,"diagnostics":[]}` — a host driver was told the cell failed and
/// given no reason at all. A session is the surface a model iterates on; "it
/// failed" with no cause is the least actionable thing a compiler can say.
///
/// A `Mutex` for the same reason `SESSION_RESULT` is one: `on_deep_stack` runs
/// the program on another thread.
static LAST_ABORT: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Record why a run aborted. Called beside each `eprintln!` so the two cannot
/// drift — a reader of either surface sees the same sentence.
fn set_last_abort(msg: String) {
    if let Ok(mut g) = LAST_ABORT.lock() {
        *g = Some(msg);
    }
}

/// Take the last abort reason, clearing it.
pub fn take_last_abort() -> Option<String> {
    LAST_ABORT.lock().ok().and_then(|mut g| g.take())
}

/// Enable in-process session capture (R44). Returns the previous setting.
pub fn set_session_capture(on: bool) -> bool {
    SESSION_CAPTURE.swap(on, std::sync::atomic::Ordering::Relaxed)
}

pub(crate) fn session_capture() -> bool {
    SESSION_CAPTURE.load(std::sync::atomic::Ordering::Relaxed)
}

/// Take the `(bindings, shapes)` produced by the last session run, clearing it.
///
/// `None` means the run did not complete successfully — which is exactly the
/// case in which the session must NOT advance (R44 §4 S4).
pub fn take_session_result() -> Option<(String, String)> {
    SESSION_RESULT.lock().ok().and_then(|mut g| g.take())
}

/// Every binding visible at the end of `main`, as `name → shape`.
///
/// Describes EVERY binding, including ones [`materialise_bindings`] had to skip:
/// a closure or an aliased dict cannot be persisted, but its structure can still
/// be described, and a name the model can see but not reuse is the case it most
/// needs told about (R44 §4 S10).
fn describe_bindings(interp: &Interp) -> String {
    let merged = merged_bindings(interp);
    let mut names: Vec<&String> = merged.keys().collect();
    names.sort();
    let mut out = String::new();
    for name in names {
        out.push_str(&format!("{name}: {}\n", value_shape(&merged[name])));
    }
    out
}

/// Globals and `main`'s final top-level locals by name, locals overriding
/// globals of the same name.
fn merged_bindings(interp: &Interp) -> HashMap<String, Value> {
    let mut merged: HashMap<String, Value> = interp
        .globals
        .iter()
        .map(|(k, v)| (sym_name(*k).to_string(), v.clone()))
        .collect();
    for (k, v) in interp.main_locals.borrow().iter() {
        merged.insert(k.clone(), v.clone());
    }
    merged
}

/// The session's bindings as Axon source literals, for the next cell's prelude.
///
/// `main`'s final top-level locals override globals of the same name — a cell
/// that mutated a binding persists the mutated value.
fn materialise_bindings(interp: &Interp) -> String {
    let merged = merged_bindings(interp);
    let mut names: Vec<&String> = merged.keys().collect();
    names.sort();

    // A `Dict` is `Rc<RefCell<..>>` — SHARED MUTABLE state. Writing two aliasing
    // bindings out as two `dict_from_pairs(..)` calls would reconstruct them as
    // two INDEPENDENT dicts, so a later `dict_set(a, ..)` would stop being
    // visible through `b`. That is a silent semantic change across a cell
    // boundary, which is exactly the class of thing this session refuses rather
    // than fudges (the same call R15 made for `Chan`). So: find every dict
    // reachable from more than one binding and skip it with a reason.
    let mut seen: HashMap<*const (), usize> = HashMap::new();
    fn count_dicts(v: &Value, seen: &mut HashMap<*const (), usize>) {
        match v {
            Value::Dict(d) => {
                let key = Rc::as_ptr(d) as *const ();
                let e = seen.entry(key).or_insert(0);
                *e += 1;
                // Descend only the FIRST time this dict is seen. A dict can
                // contain itself (`dict_set(d, "self", d)`), and an
                // unconditional recursion would not terminate — a hang at
                // session-dump time, which is worse than the aliasing bug this
                // guard exists to prevent.
                if *e == 1 {
                    for inner in d.borrow().values() {
                        count_dicts(inner, seen);
                    }
                }
            }
            Value::Array(items) => {
                for it in items.vals().iter() {
                    count_dicts(it, seen);
                }
            }
            Value::Tuple(items) => {
                for it in items.vals().iter() {
                    count_dicts(it, seen);
                }
            }
            Value::Struct(s) => {
                for (_, f) in s.fields.iter() {
                    count_dicts(f, seen);
                }
            }
            Value::Enum(e) => {
                for (_, f) in e.fields.iter() {
                    count_dicts(f, seen);
                }
            }
            Value::Some(i) | Value::Ok(i) | Value::Err(i) => count_dicts(i, seen),
            _ => {}
        }
    }
    for name in &names {
        count_dicts(&merged[*name], &mut seen);
    }
    let is_aliased = |v: &Value| -> bool {
        matches!(v, Value::Dict(d) if seen.get(&(Rc::as_ptr(d) as *const ())).copied().unwrap_or(0) > 1)
    };

    let mut lines = String::new();
    for name in names {
        // A binding that SHADOWS A BUILTIN must not persist. It is legal Axon
        // inside one cell — `let len = 5` merely warns (W0002) — but once it
        // reaches the prelude, every later cell that calls `len(xs)` dies with
        // E0306 "cannot call non-function value", and the session never
        // recovers. Measured: one task naming a variable `len` poisoned 14 of
        // the following cells in a tasks_hard run, which scored as Axon failing
        // tasks it can actually do.
        //
        // Skipping loses the value, which is why it is REPORTED — the
        // alternative is a session that silently breaks a builtin for every cell
        // after this one.
        if crate::builtins::is_known_builtin(name) {
            lines.push_str(&format!(
                "// SKIPPED {name}: shadows the builtin `{name}`; persisting it would \
                 break every later call to it\n"
            ));
            continue;
        }
        if is_aliased(&merged[name]) {
            lines.push_str(&format!(
                "// SKIPPED {name}: dict is shared with another binding; writing it out \
                 would split it into independent copies\n"
            ));
            continue;
        }
        match value_as_literal(&merged[name]) {
            Ok(lit) => lines.push_str(&format!("let {name} = {lit}\n")),
            Err(why) => lines.push_str(&format!("// SKIPPED {name}: {why}\n")),
        }
    }
    lines
}

fn run_program_inner(
    program: &Program,
    discharged: crate::verify::Discharged,
    map_status: bool,
) -> i32 {
    let mut interp = match Interp::build(program) {
        Ok(ix) => ix.with_discharged(discharged),
        Err(refused) => {
            let _ = std::io::stdout().flush();
            eprintln!("error: {refused}");
            return 2;
        }
    };
    // BUG_HUNT #23: a missing entry point is a COMPILE-time error (the program
    // is malformed), not a runtime panic. Report it cleanly with exit 2 (the
    // compile-error code) instead of `panic: no main` + exit 101 — and never
    // exit 0, which masqueraded as success.
    if !interp.fns.contains_key("main") {
        let _ = std::io::stdout().flush();
        eprintln!("error: no `main` function defined — a runnable program needs `fn main() -> i64` (or `fn main()`)");
        return 2;
    }
    let outcome = interp.init_globals().and_then(|()| interp.run_main());
    // R44 Slice 1: materialise the session's bindings. Two consumers now — the
    // AXON_DUMP_* env vars (the original prototype surface, kept so existing
    // tests and external drivers keep working) and `axon session`, which calls
    // the API rather than setting an env var on itself to talk to its own
    // interpreter. Only on success: a cell that failed must not mutate the
    // session (R44 §4 S4).
    if outcome.is_ok() {
        if session_capture() {
            let b = materialise_bindings(&interp);
            let s = describe_bindings(&interp);
            if let Ok(mut g) = SESSION_RESULT.lock() {
                *g = Some((b, s));
            }
        }
        if let Ok(spath) = std::env::var("AXON_DUMP_SHAPES") {
            let _ = std::fs::write(spath, describe_bindings(&interp));
        }
        if let Ok(path) = std::env::var("AXON_DUMP_BINDINGS") {
            let _ = std::fs::write(path, materialise_bindings(&interp));
        }
    }
    // Two different things, deliberately judged by two different rules: a status
    // the program STATED with `exit(n)` is honoured as written (the ledger
    // vocabulary is userland's to use), while a value that merely fell out of
    // `main` is an answer and may not impersonate a guard. See
    // `stated_exit_status` / `returned_exit_status`.
    let report = |(code, complaint): (i32, Option<String>)| -> i32 {
        if let Some(msg) = complaint {
            let _ = std::io::stdout().flush();
            eprintln!("{msg}");
        }
        code
    };
    match outcome {
        Ok(Value::Int(n)) if map_status => report(returned_exit_status(n)),
        Ok(Value::Int(n)) => n as i32,
        Ok(_) => 0,
        Err(Flow::Exit(code)) if map_status => report(stated_exit_status(code as i64)),
        Err(Flow::Exit(code)) => code,
        Err(Flow::VerifyFailed(msg)) => {
            // Policy rejection, not a crash — distinct exit code so CI can tell
            // "verification failed" apart from "the program panicked" (#26).
            let _ = std::io::stdout().flush();
            eprintln!("axon: verify failed: {msg}");
            VERIFY_FAILED_EXIT_CODE
        }
        Err(Flow::Halted(msg)) => {
            // The corrigibility kill-switch caught a call — refused, not crashed.
            // Distinct exit code (4) so a supervisor branches on "the switch
            // fired" specifically. (R9)
            let _ = std::io::stdout().flush();
            eprintln!("axon: halted: {msg}");
            HALTED_EXIT_CODE
        }
        Err(Flow::AiPolicyUnreachable(msg)) => {
            // AI-policy condition (E1300/E1301/E1302): offline-no-fallback,
            // budget exhausted, or unknown tier. User-actionable, not a crash —
            // distinct exit code (5) so a supervisor branches on "AI policy needs
            // attention" instead of treating it like an overflow/div0 bug.
            let _ = std::io::stdout().flush();
            set_last_abort(format!("ai policy: {msg}"));
            eprintln!("axon: ai policy: {msg}");
            AI_POLICY_EXIT_CODE
        }
        Err(Flow::Panic(msg)) => {
            let _ = std::io::stdout().flush();
            set_last_abort(format!("panic: {msg}"));
            eprintln!("axon: panic: {msg}");
            101
        }
        Err(Flow::Resume(_)) => {
            // `resume(..)` reached the top level — it was used outside a handler
            // arm (the resolver normally rejects this at check time; this is the
            // runtime backstop). A crash, not a silent exit.
            let _ = std::io::stdout().flush();
            set_last_abort("panic: `resume` called outside an effect-handler arm".to_string());
            eprintln!("axon: panic: `resume` called outside an effect-handler arm");
            101
        }
        Err(Flow::HandlerDone(..)) => {
            // A multi-shot handler's `HandlerDone` escaped its `with` block — an
            // interpreter bug (it is always caught by `eval_with_handler`). Treat
            // as a panic rather than a silent exit.
            let _ = std::io::stdout().flush();
            set_last_abort("panic: handler continuation escaped its `with` block".to_string());
            eprintln!("axon: panic: handler continuation escaped its `with` block");
            101
        }
        Err(Flow::MultiShotUnsound(msg)) => {
            // E1314: a multi-shot `resume` over a body that re-fires effects. The
            // replay-based continuation can't soundly re-execute a side effect, so
            // we refuse rather than silently double-fire or drop work. A panic-class
            // stop (the program wants true delimited continuations — deferred).
            let _ = std::io::stdout().flush();
            eprintln!("axon: multi-shot resume not supported here: {msg}");
            101
        }
        Err(Flow::RefineViolation(msg)) => {
            // A refinement-type precondition was violated by a non-constant arg.
            // Not a crash — the caller passed an out-of-contract value — so a
            // distinct exit code (6), like @[verify]→3 / @[corrigible]→4.
            let _ = std::io::stdout().flush();
            eprintln!("axon: refinement violated: {msg}");
            REFINE_VIOLATION_EXIT_CODE
        }
        Err(Flow::GoalBudgetExhausted(msg)) => {
            // R12b: a kernel Goal hit its principal's budget ceiling. Not a crash;
            // distinct exit code (7) so a supervisor can branch on it. (E1604)
            let _ = std::io::stdout().flush();
            eprintln!("axon: goal budget exhausted: {msg}");
            GOAL_BUDGET_EXIT_CODE
        }
        Err(Flow::SandboxViolation(msg)) => {
            // F5: a sandboxed call attempted an effect outside its declared ceiling.
            // Not a crash — the sandbox is doing its job — distinct exit code (8)
            // so a supervisor can branch on "the sandbox caught this" specifically.
            let _ = std::io::stdout().flush();
            eprintln!("axon: sandbox violation: {msg}");
            SANDBOX_VIOLATION_EXIT_CODE
        }
        // A stray return/break/continue escaping `main` — treat as clean exit.
        Err(_) => 0,
    }
}

/// Run a single zero-argument function (e.g. an `@[test]`) by name.
///
/// Returns `Ok(())` exactly when `axon test` passes it as a plain test: it
/// did not panic or exit non-zero, and it did not end on a failure value
/// ([`TestEnd::failure_exit`], R51 §3). Otherwise `Err(message)`.
pub fn run_test_fn(program: &Program, name: &str) -> Result<(), String> {
    match run_test_fn_outcome(program, name)?.failure_exit() {
        Some(why) => Err(why.to_string()),
        None => Ok(()),
    }
}

/// How a test that did not fail ENDED — the affirmative evidence Protected
/// Check Isolation rests on. `Completed` means the test body itself returned
/// normally, with a value that is not an `Err`. That is NOT evidence that every
/// assertion ran: an assertion inside a closure handed to code that never calls
/// it does not execute, and the test still completes (PSV-3; a suite must
/// assert after the call). A
/// test ended by `exit(0)` from below it, or one that returned `Err`, is
/// `EndedEarly`: no completion evidence is issued for it, so a check that
/// requires completion does not count it. `axon test` reports an early end
/// on a failure value ([`TestEnd::failure_exit`]) as a failure, and any
/// other early end as passing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TestEnd {
    Completed,
    EndedEarly(String),
}

/// [`TestEnd::EndedEarly`]'s reason for a test that ended on a failure value:
/// it returned `Err` (at its tail, through `return` or through `?`), or a
/// `return`/`?` ended its body early with `None`. An early `return None` and
/// a `?` on `None` leave the same trace, so both fail (S7-UnsealedParity-8).
const TEST_ENDED_ON_FAILURE: &str = "the test ended on `Err`, or early on `None` (a `return None` \
                                     or a `?` on `None`): it did not complete";

impl TestEnd {
    /// The reason, when the test ended on a failure value — an `Err`, or an
    /// early `None` (`return None` or a `?`) — rather than through `exit(0)`
    /// or a plain `return`. `axon test` fails such a test (R51 §3: a test
    /// passes only if its body completed).
    pub fn failure_exit(&self) -> Option<&str> {
        match self {
            TestEnd::EndedEarly(m) if m == TEST_ENDED_ON_FAILURE => Some(m),
            _ => None,
        }
    }
}

pub fn run_test_fn_outcome(program: &Program, name: &str) -> Result<TestEnd, String> {
    let sealed_dirs = crate::resolver::sealed_module_dirs();
    on_deep_stack(|| run_test_fn_inner(program, name, &sealed_dirs))
}

/// `sealed_dirs`: the run's sealed set ([`Interp::build_in`]).
fn run_test_fn_inner(
    program: &Program,
    name: &str,
    sealed_dirs: &[std::path::PathBuf],
) -> Result<TestEnd, String> {
    let mut interp = Interp::build_in(program, sealed_dirs)?;
    if let Err(f) = interp.init_globals() {
        return Err(flow_to_msg(f));
    }
    let Some(f) = interp.fns.get(name).copied() else {
        return Err(format!("no function `{name}`"));
    };
    interp.test_outcome(f)
}

impl<'p> Interp<'p> {
    /// Run the `@[test]` fn `f` (globals initialised) and classify how it
    /// ended: [`run_test_fn_outcome`] after the build.
    fn test_outcome(&self, f: &'p FnDef) -> Result<TestEnd, String> {
        self.test_frame_depth.set(self.call_depth.get() + 1);
        self.test_body_finished.set(false);
        match self.call_test_fn(f) {
            // COMPLETED is decided on one fact: the test's own body evaluated to
            // its end (`call_fn_in_general` records it). A `return` or a `?` is
            // an ordinary `Ok` at the test's frame, so the returned VALUE cannot
            // say whether the assertions after it ran: a `?` on a type-confused
            // `None` returned `None`, which read as a completion and was minted
            // a token (C9 round 3, PSV-3).
            Ok(Value::Err(_)) => Ok(TestEnd::EndedEarly(TEST_ENDED_ON_FAILURE.to_string())),
            Ok(Value::None) if !self.test_body_finished.get() => {
                Ok(TestEnd::EndedEarly(TEST_ENDED_ON_FAILURE.to_string()))
            }
            Ok(_) if !self.test_body_finished.get() => Ok(TestEnd::EndedEarly(
                "the test body ended early (a `return`): it did not complete".to_string(),
            )),
            Ok(_) => Ok(TestEnd::Completed),
            Err(Flow::Panic(m)) => Err(m.into()),
            // A verify failure inside a test is still a failure (drives
            // `@[test(should_fail)]`); surface its message like a panic.
            Err(Flow::VerifyFailed(m)) => Err(m.into()),
            // A corrigibility halt inside a test is a failure too (lets
            // `@[test(should_fail)]` assert the kill-switch latched).
            Err(Flow::Halted(m)) => Err(m.into()),
            // An AI-policy stop inside a test is a failure (surfaces like a panic;
            // also lets `@[test(should_fail)]` assert the policy gate fired).
            Err(Flow::AiPolicyUnreachable(m)) => Err(m.into()),
            // A refinement-precondition violation inside a test is a failure too
            // (lets `@[test(should_fail)]` assert a bad arg is caught).
            Err(Flow::RefineViolation(m)) => Err(m.into()),
            // A kernel-goal budget exhaustion inside a test is a failure too (lets
            // `@[test(should_fail)]` assert the budget ceiling fired).
            Err(Flow::GoalBudgetExhausted(m)) => Err(m.into()),
            // A sandbox violation inside a test is a failure (lets
            // `@[test(should_fail)]` assert the sandbox ceiling fired).
            Err(Flow::SandboxViolation(m)) => Err(m.into()),
            Err(Flow::Resume(_)) => {
                Err("`resume` called outside an effect-handler arm".to_string())
            }
            // E1314 multi-shot-unsound inside a test is a failure (lets
            // `@[test(should_fail)]` assert the unsound-replay case is refused).
            Err(Flow::MultiShotUnsound(m)) => Err(m.into()),
            Err(Flow::Exit(0)) => Ok(TestEnd::EndedEarly(
                "`exit(0)` ended the test before it completed".to_string(),
            )),
            Err(Flow::Exit(n)) => Err(format!("exited with code {n}")),
            // Unreachable: the test's frame ends its own `return` (and
            // `contain_frame` every other transfer). Kept so the match stays
            // total; it grants nothing.
            Err(Flow::Return(_)) => Err("`return` escaped the test's own frame".to_string()),
            // A `break` / `continue` (or an effect-handler completion) that
            // escapes a function unwinds the test BEFORE its assertions ran: the
            // test did not complete, so it did not pass. It used to count as
            // clean, which let a candidate's function end the operator's
            // acceptance test early and have Fabric sign a pass (v0.22 G01 final
            // re-audit, executed).
            Err(Flow::Break) | Err(Flow::Continue) => Err(
                "a `break`/`continue` escaped a function and unwound the test before it completed"
                    .to_string(),
            ),
            Err(Flow::HandlerDone(..)) => {
                Err("an effect handler completed outside its handled computation".to_string())
            }
        }
    }
}

fn flow_to_msg(f: Flow) -> String {
    match f {
        Flow::Panic(m)
        | Flow::VerifyFailed(m)
        | Flow::Halted(m)
        | Flow::AiPolicyUnreachable(m)
        | Flow::RefineViolation(m)
        | Flow::GoalBudgetExhausted(m)
        | Flow::SandboxViolation(m) => m.into(),
        Flow::Exit(n) => format!("exited with code {n}"),
        _ => "non-local control flow escaped the program".into(),
    }
}

/// Founder-facing label for a `@[verify]`-armed function in failure messages.
/// `assert_deployable` is the *generated* deploy-gate symbol the surface
/// compiler emits (see axon-surface `compile.rs`); leaking that name to a
/// non-technical user is an impl-detail leak (BUG_HUNT #25). Map it to plain
/// language; any author-named verify fn keeps its own name (the author chose
/// it, so it's meaningful to them).
fn verify_fn_label(fn_name: &str) -> String {
    if fn_name == "assert_deployable" {
        "the deploy gate".to_string()
    } else {
        format!("`{fn_name}`")
    }
}

impl<'p> Interp<'p> {
    /// Build the interpreter for `program`.
    ///
    /// `Err` (an [`EngineRefusal`]'s message) when the run is sealed and must
    /// not execute: this build's runtime seal is incomplete
    /// ([`SEALED_RUN_REFUSED`]), the VM is requested
    /// ([`SEALED_RUN_VM_REFUSED`]) or `AXON_ENGINE` is invalid (R51 §4 Fork
    /// 1, §7). Every run entry passes through here, so no sealed run executes
    /// unprotected or on the VM, whoever spawned it.
    pub fn build(program: &'p Program) -> Result<Self, String> {
        Self::build_in(program, &crate::resolver::sealed_module_dirs())
    }

    /// [`Interp::build`] for the sealed set `sealed_dirs` (the process's
    /// `resolver::sealed_module_dirs()` there): the one input both the
    /// refusal and the seal state are computed from. `engine_for` decides a
    /// sealed run before it looks at the override or `AXON_ENGINE`. Taking
    /// the set as a value lets a test build a sealed run without publishing
    /// it to every concurrent build in the process.
    fn build_in(program: &'p Program, sealed_dirs: &[std::path::PathBuf]) -> Result<Self, String> {
        let engine =
            vm::engine_at_build(!sealed_dirs.is_empty()).map_err(|refused| refused.to_string())?;
        Ok(Self::build_with(program, sealed_dirs, engine))
    }

    /// [`Interp::build`] with the sealed set (`sealed_dirs`, empty for an
    /// unsealed run) and the engine already decided (`engine` is
    /// [`vm::engine_for`]'s choice for that run). A sealed run computes its
    /// [`Seal`] and its pins here, once; an unsealed run computes neither.
    fn build_with(
        program: &'p Program,
        sealed_dirs: &[std::path::PathBuf],
        engine: Engine,
    ) -> Self {
        pin_ai_net_allowlist(program);
        let (seal, pins) = if sealed_dirs.is_empty() {
            (Seal::default(), pin::Pins::default())
        } else {
            let sealed = |sp: crate::span::Span| crate::resolver::span_in_sealed(sp, sealed_dirs);
            (
                Seal::of(program, &sealed),
                pin::Pins::build(program, &sealed),
            )
        };
        let mut fns = HashMap::new();
        let mut structs = HashMap::new();
        let mut enums = HashMap::new();
        let mut methods = HashMap::new();
        let mut global_defs = Vec::new();
        let mut refine_preds = HashMap::new();
        let mut user_traits = std::collections::HashSet::new();
        let mut trait_impls = std::collections::HashSet::new();
        let mut refine_bases = HashMap::new();
        let mut impl_of = HashMap::new();

        for item in &program.items {
            match item {
                Item::FnDef(f) => {
                    fns.insert(f.name.clone(), f);
                }
                Item::RefineDef(r) => {
                    // Phase 5: index the predicate so `call_fn` can evaluate it as
                    // a runtime precondition when a param's type is this refinement.
                    refine_preds.insert(r.name.clone(), r.predicate.as_ref());
                    refine_bases.insert(r.name.clone(), &r.base);
                }
                Item::TraitDef(t) => {
                    user_traits.insert(t.name.clone());
                }
                Item::TypeDef(t) => {
                    structs.insert(t.name.clone(), t);
                }
                Item::EnumDef(e) => {
                    enums.insert(e.name.clone(), e);
                }
                Item::ImplBlock(
                    b @ ImplBlock {
                        trait_name,
                        for_type,
                        methods: ms,
                        ..
                    },
                ) => {
                    let tn = type_name_of(for_type);
                    if !trait_name.is_empty() {
                        trait_impls.insert((tn.clone(), trait_name.clone()));
                    }
                    for m in ms {
                        methods.insert((tn.clone(), m.name.clone()), m);
                        if seal.active {
                            impl_of.insert(m as *const FnDef as usize, b);
                        }
                    }
                }
                Item::LetDef { name, value, .. } => {
                    global_defs.push((name.clone(), value.as_ref()));
                }
                _ => {}
            }
        }

        // AX-18: fns and methods with their parameter syms, indexed by name
        // sym (fns only — a method is never called by bare name) and by def.
        let res = Resolution::build(program, &structs, &enums);
        let mut fn_table = Vec::new();
        let mut fn_of_sym = FxHashMap::default();
        let mut fn_of_def = FxHashMap::default();
        for item in &program.items {
            let defs: &[FnDef] = match item {
                Item::FnDef(f) => std::slice::from_ref(f),
                Item::ImplBlock(blk) => &blk.methods,
                _ => continue,
            };
            for f in defs {
                let idx = u32::try_from(fn_table.len()).expect("fewer than 2^32 fns");
                if matches!(item, Item::FnDef(_))
                    && fns
                        .get(&f.name)
                        .is_some_and(|g: &&FnDef| std::ptr::eq(*g, f))
                {
                    fn_of_sym.insert(intern(&f.name), idx);
                }
                fn_of_def.insert(f as *const FnDef as usize, idx);
                // R50: a table entry is the only kind whose body is compiled.
                fn_table.push(FnEntry {
                    compiled: Some(std::cell::OnceCell::new()),
                    hot: res.hot_fn(f),
                    index: idx,
                    ..FnEntry::new(f, &fns, seal.active)
                });
            }
        }

        Interp {
            res,
            pure_slots: fn_table.iter().map(|_| vm::PureSlot::default()).collect(),
            fn_table,
            fn_of_sym,
            fn_of_def,
            fns,
            structs,
            enums,
            methods,
            global_defs,
            globals: FxHashMap::default(),
            kernels: {
                // Read the ambient effect ceiling once (its parse warns on
                // stderr); both kernels start under the SAME ceiling.
                let ambient = ambient_sandbox();
                [
                    Kernel::new(false, ambient.clone()),
                    Kernel::new(true, ambient),
                ]
            },
            call_depth: Cell::new(0),
            test_frame_depth: Cell::new(0),
            test_body_finished: Cell::new(false),
            #[cfg(target_arch = "wasm32")]
            nest: Cell::new(0),
            #[cfg(target_arch = "wasm32")]
            nest_limit: resolve_max_depth().saturating_mul(NEST_PER_DEPTH),
            max_depth: resolve_max_depth(),
            seal,
            pins,
            frame_sealed: Cell::new(false),
            pin_fn: Cell::new(0),
            sealed_frames: Cell::new(0),
            operator_frames: Cell::new(0),
            taint: taint::Taint::default(),
            user_traits,
            trait_impls,
            refine_bases,
            chan_contracts: RefCell::new(HashMap::new()),
            dict_snaps: RefCell::new(HashMap::new()),
            dict_epoch: Cell::new(0),
            impl_of,
            fn_cx_cache: RefCell::new(HashMap::new()),
            engine,
            vm_trace: vm::trace_at_build(),
            vm_eager: vm::eager_at_build(),
            enclosing_agent: RefCell::new(None),
            current_goal: RefCell::new(None),
            current_fn: Cell::new(None),
            current_call_tier: RefCell::new(None),
            callees: RefCell::new(Vec::new()),
            ai_calls_this_fn: Cell::new(0),
            arg_bufs: RefCell::new(Vec::new()),
            env_pool: RefCell::new(Vec::new()),
            frame_spare: Cell::new(None),
            pure_regs: Cell::new(Vec::new()),
            pure_frames: Cell::new(Vec::new()),
            pure_replay: Cell::new(false),
            ai_cost_micro: Cell::new(0),
            w1310_warned: RefCell::new(std::collections::HashSet::new()),
            tokens_used: Cell::new(0),
            token_budget: parse_token_budget(),
            handlers: RefCell::new(Vec::new()),
            resume_replay: RefCell::new(None),
            resume_ctx: RefCell::new(Vec::new()),
            refine_preds,
            main_locals: RefCell::new(HashMap::new()),
            discharged: crate::verify::Discharged::default(),
            gfx_mock: RefCell::new(crate::native::GfxMock::new()),
            #[cfg(not(target_arch = "wasm32"))]
            domain: axon_domain::Registry::new(),
            #[cfg(feature = "gfx-wgpu")]
            gfx_real: RefCell::new(axon_gfx::GfxReal::new()),
        }
    }

    /// Install the set of statically-discharged obligations (Phase 5 §4). A
    /// pipeline that ran the SMT prover passes its `Discharged` here so the
    /// interpreter elides the runtime checks Z3 already proved ∀-inputs. A no-op
    /// for any obligation not in the set, so this only ever *removes* a check
    /// that could not have fired.
    pub fn with_discharged(mut self, discharged: crate::verify::Discharged) -> Self {
        self.discharged = discharged;
        self
    }

    /// Whether `f` was defined in a sealed (candidate) module of this run.
    /// Read by the S2 taint and the S3 call crossing (R51 §11).
    pub(crate) fn fn_is_sealed(&self, f: &FnDef) -> bool {
        self.seal.active && self.seal.fns.contains(&(f as *const FnDef as usize))
    }

    /// What sealed code is told when a name does not resolve FOR IT: the
    /// operator's fn and a fn that does not exist read the same, so a sealed
    /// caller cannot use the refusal to learn which names the operator defines
    /// (existence oracle, C9 round 10).
    pub(crate) fn sealed_no_fn_msg(name: &str) -> String {
        format!(
            "sealed code (the candidate under test) cannot use `{name}`: no such function or \
             value is visible to it"
        )
    }

    /// The error for a name that resolves to nothing: the common text above for
    /// a sealed caller, `plain` for anyone else.
    pub(crate) fn no_such_fn<T>(&self, name: &str, plain: String) -> Result<T, Flow> {
        if self.seal.active && self.frame_sealed.get() {
            return panic(Self::sealed_no_fn_msg(name));
        }
        panic(plain)
    }

    /// Whether handler `frame` may answer (or abort) the operation being
    /// performed now. An operator frame may answer anything. A SEALED frame
    /// may answer only while no operator frame has been entered since it was
    /// installed: otherwise the operation belongs to — or unwinds through —
    /// the operator's code, and the candidate would choose its result or skip
    /// the rest of it (C9 round 3, PSV-1). The single predicate for live
    /// frames (`run_handler_arm`) and the replay feed.
    pub(crate) fn handler_may_answer(&self, sealed: bool, operator_frames: usize) -> bool {
        !sealed || operator_frames == self.operator_frames.get()
    }
    /// Run `g` with the frame's provenance set to `sealed`, restoring it
    /// after: the ONE place provenance changes, so the per-provenance frame
    /// counts stay exact. The S3 seal edges enter frames through it.
    pub(crate) fn with_frame<T>(&self, sealed: bool, g: impl FnOnce() -> T) -> T {
        let prev = self.frame_sealed.replace(sealed);
        let count = if sealed {
            &self.sealed_frames
        } else {
            &self.operator_frames
        };
        count.set(count.get() + 1);
        if sealed {
            self.taint.entries.set(self.taint.entries.get() + 1);
        }
        let out = g();
        count.set(count.get() - 1);
        self.frame_sealed.set(prev);
        out
    }

    /// The kernel of the frame now running: the operator's, or the sealed
    /// candidate's own. Handle-addressed state is only ever reached through
    /// it. `frame_sealed` is never set in an unsealed run, so that run only
    /// ever reaches `kernels[0]`.
    #[inline(always)]
    fn k(&self) -> &Kernel {
        &self.kernels[usize::from(self.frame_sealed.get())]
    }

    /// The ONLY way to draw a random number: from the running frame's kernel.
    /// A sealed frame draws from the candidate's own stream and so can never
    /// advance or observe the operator's (PSV-1).
    ///
    /// OPERATOR code running ABOVE a sealed frame (a closure the suite handed
    /// the candidate, an operator handler arm answering a candidate operation)
    /// is REFUSED: the candidate decides how many times such code runs, so
    /// every draw it made would advance the operator's stream by a count the
    /// candidate chose, and steer the operator's next challenge (C9 round 3,
    /// PSV-1, amendment 46).
    pub(crate) fn rng_next(&self) -> Result<u64, Flow> {
        self.rng_guard()?;
        Ok(self.k().rng_next())
    }

    /// The ONLY way to reseed (`srand`): the running frame's kernel only,
    /// under the same refusal as [`Interp::rng_next`].
    pub(crate) fn rng_reseed(&self, n: i64) -> Result<(), Flow> {
        self.rng_guard()?;
        self.k().rng_set(n);
        Ok(())
    }

    fn rng_guard(&self) -> Result<(), Flow> {
        if !self.frame_sealed.get() && self.sealed_frames.get() > 0 {
            return panic(
                "the operator's random stream cannot be used by operator code that sealed code \
                 (the candidate under test) is running — the candidate would choose how far it \
                 advances; draw before handing the code to the candidate",
            );
        }
        Ok(())
    }

    /// The NAME rule's edge (C9 round 10, amendment 100): operator code does not
    /// hand a name-resolving builtin (`sandbox_run`, `scheduler_spawn`,
    /// `goal_eval` ...) a name the candidate could have chosen. A `str` the
    /// candidate returned is a perfectly good `str`, and the builtin then ran
    /// whichever operator fn it named. Judged at the CALL SITE, by the static
    /// name-purity analysis (`interp/pin.rs`): an operator literal, constant or
    /// operator fn result passes; a candidate fn's result, a parameter or any
    /// untyped read does not. Fail-closed: an unrecorded site is refused.
    pub(crate) fn seal_name_args(&self, builtin: &str, args: &[Expr]) -> Result<(), Flow> {
        if !self.seal.active || self.frame_sealed.get() {
            return Ok(());
        }
        let Some(idxs) = pin::name_sink(builtin) else {
            return Ok(());
        };
        #[cfg(test)]
        if DISPATCH_RULE_OFF.with(Cell::get) {
            return Ok(());
        }
        for &i in idxs {
            if let Some(a) = args.get(i) {
                if !self.pins.name_determined(self.pin_fn.get(), a) {
                    return panic(format!(
                        "operator code gave `{builtin}` a function name nothing on the operator \
                         side determined (argument {}) — the candidate would choose which \
                         function runs; name it with a literal or a constant the operator wrote",
                        i + 1
                    ));
                }
            }
        }
        Ok(())
    }

    /// The method-dispatch edge: in operator code, a method name the operator
    /// defines is the operator's. A method call selects its method by the
    /// receiver's RUNTIME type, which the candidate chooses (its declared
    /// return type, or a type confusion), so without this edge the candidate
    /// chose which code ran under the operator's judging method's name — the
    /// C9 round-4 review's keyed pass. A candidate's OWN method names (its
    /// API, which the suite may call) are not affected.
    pub(crate) fn seal_method(&self, f: &FnDef, tn: &str) -> Result<(), Flow> {
        if self.seal.active
            && !self.frame_sealed.get()
            && self.fn_is_sealed(f)
            && self.seal.operator_methods.contains(&f.name)
        {
            return panic(format!(
                "operator code called `.{}()`, a method the operator defines, on a value of type \
                 `{tn}` whose `{}` is the candidate's — the candidate would choose the code that \
                 runs under the operator's method",
                f.name, f.name
            ));
        }
        Ok(())
    }

    /// The DISPATCH rule (C9 round 6, amendment 83): in a sealed run, operator
    /// code does not select an operator impl's method by the runtime type of a
    /// receiver nothing on the operator side determined (`interp/pin.rs`). The
    /// impl is chosen by that type, which for a value read from a dict, a
    /// channel, an unannotated lambda parameter or an unbound generic position
    /// is the candidate's choice. Applies only where there is an impl to choose
    /// between (two or more operator impl types define the method). The
    /// runtime form is [`Interp::t_check_dispatch`] (R51 S4).
    pub(crate) fn seal_dispatch(
        &self,
        receiver: &Expr,
        f: &FnDef,
        recv: &Value,
        tn: &str,
    ) -> Result<(), Flow> {
        if !self.seal.active || self.frame_sealed.get() || self.fn_is_sealed(f) {
            return Ok(());
        }
        // Tests of the OTHER seal layers observe their attacks through a dispatch
        // on an untyped read - exactly what this rule refuses first. They run
        // with the rule off (as a paired-disable cell does) so each layer is
        // judged by its own attack; the rule's own tests run with it on.
        #[cfg(test)]
        if DISPATCH_RULE_OFF.with(Cell::get) {
            return Ok(());
        }
        if !self.pins.selects_between_impls(&f.name) {
            return Ok(());
        }
        // A receiver that is an operator-defined struct or enum selects an
        // impl the operator chose: sealed code cannot build one (E0004).
        let operator_value = match recv {
            Value::Struct(s) => self.pins.is_operator_type(&sym_name(s.name)),
            Value::Enum(e) => self.pins.is_operator_type(&sym_name(e.enum_name)),
            _ => false,
        };
        if operator_value {
            return Ok(());
        }
        if !self.pins.determined(self.pin_fn.get(), receiver, &f.name) {
            return panic(format!(
                "operator code dispatched `{}` on a value whose type nothing on the operator side \
                 determined (here `{tn}`) — the candidate would choose the impl; pin it with \
                 `let x: T = ...`",
                f.name
            ));
        }
        Ok(())
    }

    /// The dispatch rule's arithmetic arm (amendment 83): operator code does
    /// not do arithmetic on a fixed-width integer whose WIDTH nothing on the
    /// operator side determined — a `u8` the candidate chose wraps where the
    /// operator's `i64` does not (`(v << 1) == 254` with `v = 255 as u8`).
    pub(crate) fn seal_width(
        &self,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
        l: &Value,
        r: &Value,
    ) -> Result<(), Flow> {
        if !self.seal.active || self.frame_sealed.get() {
            return Ok(());
        }
        if !(width_sized(l) || width_sized(r)) {
            return Ok(());
        }
        if matches!(
            op,
            BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq
        ) {
            return Ok(());
        }
        #[cfg(test)]
        if DISPATCH_RULE_OFF.with(Cell::get) {
            return Ok(());
        }
        if !self
            .pins
            .determined_arith(self.pin_fn.get(), op, left, right)
        {
            return panic(format!(
                "operator code did arithmetic on a fixed-width integer whose width nothing on the \
                 operator side determined ({} {:?} {}) — the candidate would choose the \
                 wrapping; pin it with `let x: T = ...`",
                l.type_name(),
                op,
                r.type_name()
            ));
        }
        Ok(())
    }

    /// The arithmetic arm's UNARY form: `-x` and `~x` on a fixed-width integer
    /// wrap at a width the candidate chose exactly as `x + y` does, and the
    /// binary arm never saw them (C9 round 9 sweep: `(-xs[0]) == 252` completed
    /// for a candidate `4 as u8`).
    pub(crate) fn seal_width_unary(
        &self,
        op: &UnaryOp,
        operand: &Expr,
        v: &Value,
    ) -> Result<(), Flow> {
        if !self.seal.active || self.frame_sealed.get() {
            return Ok(());
        }
        if !width_sized(v) || !matches!(op, UnaryOp::Neg | UnaryOp::BitNot) {
            return Ok(());
        }
        #[cfg(test)]
        if DISPATCH_RULE_OFF.with(Cell::get) {
            return Ok(());
        }
        if !self.pins.determined_unary(self.pin_fn.get(), op, operand) {
            return panic(format!(
                "operator code did arithmetic on a fixed-width integer whose width nothing on the \
                 operator side determined ({op:?} {}) — the candidate would choose the wrapping; \
                 pin it with `let x: T = ...`",
                v.type_name()
            ));
        }
        Ok(())
    }

    /// The global-read edge: a sealed frame may not read an operator global.
    pub(crate) fn seal_global(&self, s: Sym) -> Result<(), Flow> {
        if self.seal.active && self.frame_sealed.get() && !self.seal.globals.contains(&s) {
            return panic(Self::sealed_no_fn_msg(&sym_name(s)));
        }
        Ok(())
    }

    /// Whether `s` is a module-level `let`: existence only, no value.
    #[inline(always)]
    pub(crate) fn is_global(&self, s: Sym) -> bool {
        self.globals.contains_key(&s)
    }

    /// THE ONLY read of a module-level `let` by name from running code: the
    /// global-read edge ([`Interp::seal_global`]) applied at the one lookup, so
    /// no arm (a receiver fast path, an index fast path, a closure-constant
    /// call) can reach `self.globals` around it (C9 round 9: `TABLE[0]`,
    /// `CFG.k`, `PAIR.0` read an operator global from sealed code because two
    /// fast paths skipped the edge). `Ok(None)`: no such global.
    /// Drift: `every_global_read_goes_through_global_ref`.
    #[inline(always)]
    pub(crate) fn global_ref(&self, s: Sym) -> Result<Option<&Value>, Flow> {
        match self.globals.get(&s) {
            Some(v) => {
                if self.seal.active {
                    self.seal_global(s)?;
                }
                Ok(Some(v))
            }
            None => Ok(None),
        }
    }

    /// The refinement edge: a candidate's refinement never runs in operator
    /// code (a refinement attaches BY NAME, so a candidate `type DictTable =
    /// … where P` ran P inside the operator's helper — PCI candidate-3 review).
    pub(crate) fn seal_refine(&self, rname: &str) -> Result<(), Flow> {
        if self.seal.active && !self.frame_sealed.get() && self.seal.refines.contains(rname) {
            return panic(format!(
                "the refinement `{rname}` is the candidate's and cannot run in the operator's code"
            ));
        }
        Ok(())
    }

    /// Look up a function by NAME for a builtin that will run it (scheduler,
    /// goal, sandbox …). The call edge applies at RESOLUTION time, so a name a
    /// sealed frame queues cannot run later on the operator's behalf.
    pub(crate) fn fn_by_name(&self, name: &str) -> Result<Option<&'p FnDef>, Flow> {
        match self.fns.get(name).copied() {
            Some(f) => {
                self.seal_call(f)?;
                Ok(Some(f))
            }
            None => Ok(None),
        }
    }

    /// Whether struct type `name` was defined in a sealed module.
    pub(crate) fn seal_type(&self, name: &str) -> bool {
        self.seal.active && self.seal.types.contains(name)
    }

    /// Evaluate module-level constants in source order, so each may reference
    /// those defined before it. Populates [`Interp::globals`].
    fn init_globals(&mut self) -> Result<(), Flow> {
        if self.global_defs.is_empty() {
            return Ok(());
        }
        let defs = std::mem::take(&mut self.global_defs);
        if self.seal.active {
            return self.init_globals_sealed(&defs);
        }
        let mut env = self.new_env();
        for (name, expr) in &defs {
            let v = self.eval(expr, &mut env)?;
            env.define(intern(name), v, 0);
        }
        self.globals = env.snapshot().into_parts().0.into_iter().collect();
        Ok(())
    }

    /// [`Interp::init_globals`] in a sealed run (R51 S3, veto interp.rs:
    /// 3292-3303). Each initialiser runs under its own definition's
    /// provenance (a candidate's `let` is a sealed frame: the call edge, its
    /// own kernel, and its value tainted ALL), in a FRESH environment: an
    /// earlier global is reached through `global_ref`, so the global-read
    /// edge sees every read (a shared env let a later initialiser read an
    /// earlier global as a LOCAL, past the edge; PCI candidate-4 review). The
    /// let carries the taint its initialiser was computed with
    /// (`Interp::t_global`).
    #[cold]
    #[inline(never)]
    fn init_globals_sealed(&mut self, defs: &[(String, &'p Expr)]) -> Result<(), Flow> {
        for (name, expr) in defs {
            let s = intern(name);
            let sealed = self.seal.globals.contains(&s);
            let mut env = self.new_env();
            let v = self.with_frame(sealed, || self.eval(expr, &mut env))?;
            self.t_set_global(s, self.taint.last.get());
            self.globals.insert(s, v);
        }
        Ok(())
    }

    /// Run `main` with no arguments.
    fn run_main(&self) -> R {
        match self.fns.get("main") {
            Some(f) => self.call_fn(f, vec![]),
            None => panic("no `main` function"),
        }
    }

    // ── Function / closure calls ─────────────────────────────────────────────

    /// R3 §3.3: the `@[ai(policy(fallback: "…"))]` value declared on the
    /// currently-executing fn, if any. Accepts both the grouped form and the
    /// flat `@[ai(fallback: "…")]` (the parser flattens the group), so the arg
    /// reads as `"fallback: <value>"`. Returns the fallback string when present.
    /// This is what lets an offline `ai_complete` stay total instead of panicking.
    /// Only read on the `#[cfg(not(asi-runtime))]` offline branch — when the live
    /// model is compiled in, there is no offline fallback path, so it is dead there.
    #[cfg_attr(feature = "asi-runtime", allow(dead_code))]
    fn current_ai_fallback(&self) -> Option<String> {
        let f = self.current_fn_def()?;
        let ai = f.attrs.iter().find(|a| a.name == "ai")?;
        for arg in &ai.args {
            if let Some(rest) = arg.strip_prefix("fallback:") {
                return Some(rest.trim().to_string());
            }
        }
        None
    }

    /// R3c: the `@[ai(policy(budget: N))]` ceiling declared on the
    /// currently-executing fn, if any. Returns `Some(N)` for a well-formed
    /// non-negative integer; `None` when no `budget:` field is present **or** the
    /// value is malformed (in which case a `W1311` is emitted once and the fn
    /// runs unmetered — a bad budget must never silently enforce a wrong number).
    fn current_ai_budget(&self) -> Option<u64> {
        let f = self.current_fn_def()?;
        let name = &f.name;
        // Parsed by the SHARED `budget_from_attrs`, so "is this fn metered?" has
        // exactly one answer here and in the native codegen refusal (F141).
        match crate::ai_routing::budget_from_attrs(&f.attrs)? {
            Ok(n) => Some(n),
            Err(raw) => {
                eprintln!(
                    "warning: [{}] @[ai(policy(budget: {raw}))] on `{name}` is not a \
                     non-negative integer — ignored (fn runs unmetered)",
                    crate::error::W1311
                );
                None
            }
        }
    }

    /// R4: the name of the currently-executing fn if it is in the `@[agent]`
    /// zone, else `None`. Used to inject the mandatory agent action log: every
    /// capability-bearing action an agent takes is audited (I-13).
    fn current_agent_fn(&self) -> Option<String> {
        // R4/I-13: the nearest ENCLOSING `@[agent]` on the call stack — so a
        // capability builtin called from a helper of an agent is still logged to
        // that agent's action trail (the audit can't be escaped by indirection).
        self.enclosing_agent.borrow().clone()
    }

    /// F3: enter goal-optimization scope. Records `name` as the current goal until
    /// the returned guard drops, so an `ai_complete` invoked inside a metric
    /// evaluation is attributed to the goal that triggered it. Nesting-safe (the
    /// previous goal is restored on drop). Call at the top of each `run_goal*`.
    fn enter_goal(&self, name: &str) -> FnNameOptGuard<'_> {
        FnNameOptGuard {
            cell: &self.current_goal,
            prev: self.current_goal.replace(Some(name.to_string())),
        }
    }

    /// The goal/metric name currently being optimized, or `None` outside any
    /// `run_goal*`. Stamped into the `ai_call` provenance for causal attribution.
    fn current_goal_name(&self) -> Option<String> {
        self.current_goal.borrow().clone()
    }

    /// F3 (Phase 9): the name of the principal currently in scope for audit
    /// attribution. Defaults to "root"; overridden by `principal_activate`.
    fn current_principal_name(&self) -> String {
        self.k().current_principal.borrow().clone()
    }

    /// Whether the currently-executing fn carries an `@[ai(policy)]` attribute.
    /// Used for W1310: a live/mock AI call from a fn with no policy is allowed
    /// but un-metered and un-pinned, so it warns (R3 §6).
    fn current_fn_has_ai_policy(&self) -> bool {
        self.current_fn_def()
            .map(|f| f.attrs.iter().any(|a| a.name == "ai"))
            .unwrap_or(false)
    }

    /// The fn named like the currently-executing one in the by-name fn map —
    /// the policy source for the `current_ai_*` readers.
    fn current_fn_def(&self) -> Option<&'p FnDef> {
        self.fns.get(self.current_fn.get()?.name.as_str()).copied()
    }

    /// R3 §4.2 — resolve the AI tier for the current call from the enclosing
    /// `@[ai(policy(tier: …))]`, defaulting to [`crate::ai_routing::DEFAULT_TIER`]
    /// when the fn has no policy or its policy names no tier. (Per-call `tier:`
    /// args — step 1 — are deferred until named-arg call syntax lands; this
    /// covers steps 2-3.) An *unknown* tier name in the policy is **E1302**.
    fn current_ai_tier(&self) -> Result<crate::ai_routing::Tier, Flow> {
        use crate::ai_routing::{Tier, DEFAULT_TIER};
        // R3b — step 1: a per-call `tier:` arg overrides the policy/default.
        // `take` it so it applies to exactly this one call and never leaks to a
        // nested or subsequent call.
        if let Some(raw) = self.current_call_tier.borrow_mut().take() {
            return match Tier::parse(&raw) {
                Some(t) => Ok(t),
                None => Err(Flow::AiPolicyUnreachable(
                    format!(
                        "[{}] unknown AI tier `{raw}` — configured tiers: {}",
                        crate::error::E1302,
                        Tier::configured()
                    )
                    .into(),
                )),
            };
        }
        // Steps 2-3: the enclosing @[ai(policy(tier:))], else the default —
        // resolved via the shared `ai_routing::tier_from_attrs` so the interp and
        // the native codegen refusal agree on a fn's tier exactly.
        let Some(f) = self.current_fn_def() else {
            return Ok(DEFAULT_TIER);
        };
        crate::ai_routing::tier_from_attrs(&f.attrs).map_err(|raw| {
            Flow::AiPolicyUnreachable(
                format!(
                    "[{}] unknown AI tier `{raw}` — configured tiers: {}",
                    crate::error::E1302,
                    Tier::configured()
                )
                .into(),
            )
        })
    }

    /// May the runtime persist a provenance row right now?
    ///
    /// Asks the SAME question `pre_effect_gate` asks for a builtin, through the
    /// same predicate, so the ceiling cannot mean one thing for `write_file`
    /// and another for the telemetry writer. `IO` is the row the effect
    /// catalog gives filesystem and console builtins.
    fn provenance_write_permitted(&self) -> bool {
        let handle = self.k().active_sandbox.get();
        if handle < 0 {
            return true; // no ceiling in force
        }
        let sbs = self.k().sandboxes.borrow();
        match sbs.get(handle as usize) {
            Some(sb) => {
                crate::interp::builtins::first_effect_outside_ceiling(sb, &["IO"]).is_none()
            }
            // UNREACHABLE BY CONSTRUCTION, and deliberately fail-closed
            // anyway. `active_sandbox` only ever holds -1 or an index that
            // was valid when it was stored, so there is no way to observe
            // this arm — a mutation flipping it to `true` survives the suite,
            // which is the honest signal that it is untested rather than a
            // test gap to paper over with a contrived fixture.
            //
            // Worth recording that `pre_effect_gate` diverges here: its
            // `if let Some(sb) = sbs.get(..)` has no else, so in this same
            // impossible state it SKIPS the ceiling check entirely and fails
            // OPEN. If the invariant on `active_sandbox` is ever weakened,
            // that is the site that turns into a hole, not this one.
            None => false,
        }
    }

    /// The call edge: a sealed frame may run only sealed functions.
    pub(crate) fn seal_call(&self, f: &FnDef) -> Result<(), Flow> {
        if self.seal.active && self.frame_sealed.get() && !self.fn_is_sealed(f) {
            return panic(Self::sealed_no_fn_msg(&f.name));
        }
        Ok(())
    }

    fn call_fn(&self, f: &'p FnDef, args: impl CallArgs) -> R {
        nest_guard!(self, CALL_FN);
        match self.fn_of_def.get(&(f as *const FnDef as usize)) {
            Some(&i) => self.call_fn_entry(&self.fn_table[i as usize], args),
            None => self.call_fn_entry(&FnEntry::new(f, &self.fns, self.seal.active), args),
        }
    }

    /// [`Interp::call_fn`] with the callee already resolved (AX-18: a call
    /// by name reaches its `fn_table` entry without hashing).
    fn call_fn_entry(&self, f: &FnEntry<'p>, args: impl CallArgs) -> R {
        nest_guard!(self, CALL_FN_ENTRY);
        // A `&mut` param must be moved back to the caller (`call_mut`); a
        // path that cannot do that (a fn reached by name string, a method)
        // would silently drop the callee's writes — refuse instead.
        if f.has_ref_mut {
            return panic(format!(
                "`{}` takes `&mut` parameters and can only be called directly as `{}(&mut a, ...)`",
                f.def.name, f.def.name
            ));
        }
        let mut env = self.take_frame();
        let result = self.call_fn_in(f, args, &mut env);
        self.give_frame(env);
        result
    }

    /// Run the `@[test]` fn `f` with no arguments, always on
    /// [`Interp::call_fn_in_general`]: the one path that records whether the
    /// test frame's body ran to its end (`test_body_finished`), so the
    /// common call path carries no test check (cost only).
    fn call_test_fn(&self, f: &'p FnDef) -> R {
        let owned;
        let entry = match self.fn_of_def.get(&(f as *const FnDef as usize)) {
            Some(&i) => &self.fn_table[i as usize],
            None => {
                owned = FnEntry::new(f, &self.fns, self.seal.active);
                &owned
            }
        };
        let mut env = self.take_frame();
        let result = match self.enter_fn(f) {
            Ok(_guard) => self.call_fn_in_general(entry, Vec::new(), &mut env),
            Err(flow) => Err(flow),
        };
        self.give_frame(env);
        result
    }

    /// AX-54: an empty argument vector with room for `n`, reusing one that a
    /// finished call handed back to [`Interp::recycle_args`].
    #[inline]
    fn take_args(&self, n: usize) -> Vec<Value> {
        match self.arg_bufs.borrow_mut().pop() {
            Some(mut v) => {
                v.reserve(n);
                v
            }
            None => Vec::with_capacity(n),
        }
    }

    /// Keep an emptied argument vector for [`Interp::take_args`]. Buffers are
    /// only created while the pool is empty, so it holds at most as many as
    /// were outstanding at once (argument lists nested in argument lists);
    /// the cap bounds what deep recursion through such nesting leaves behind.
    #[inline]
    fn recycle_args(&self, args: Vec<Value>) {
        debug_assert!(args.is_empty());
        let mut pool = self.arg_bufs.borrow_mut();
        if args.capacity() != 0 && pool.len() < 64 {
            pool.push(args);
        }
    }

    fn call_fn_in(&self, entry: &FnEntry<'p>, args: impl CallArgs, env: &mut Env) -> R {
        nest_guard!(self, CALL_FN_IN);
        let _guard = self.enter_fn(entry.def)?;
        // A fn with an attribute-driven step around its body, and every fn of
        // a program that declares refinements, takes the general path. The
        // common call is the steps below and nothing else; the rest stays out
        // of its frame (cost only, AX-54).
        if !entry.plain || !self.refine_preds.is_empty() {
            return self.call_fn_in_general(entry, args.into_vec(self), env);
        }
        self.call_fn_common(entry, args, env)
    }

    /// The depth check and the `current_fn` swap of every call. Bound
    /// recursion: a graceful panic instead of a process-aborting stack
    /// overflow on runaway/infinite recursion. The guard restores the depth
    /// on any return path (including `?`), and the caller's `current_fn`:
    /// the executing fn is tracked so builtins (R3 ai_call provenance) can
    /// attribute their records to the caller.
    #[inline(always)]
    fn enter_fn(&self, f: &'p FnDef) -> Result<CallGuard<'_, 'p>, Flow> {
        let depth = self.call_depth.get() + 1;
        if depth > self.max_depth {
            return Err(self.depth_exceeded(f));
        }
        self.call_depth.set(depth);
        Ok(CallGuard {
            interp: self,
            prev_fn: self.current_fn.replace(Some(f)),
        })
    }

    /// AX-56: charges `W` bytes (a `nest_cost` frame) against the wasm32
    /// stack budget, or the recursion-limit panic when that would exceed it.
    /// The call-depth guard counts fn calls, but the wasm engine's native
    /// stack holds every interpreter frame, and how many frames one call
    /// level takes grows with how deeply the call sits inside expressions,
    /// `with` bodies, callbacks and lambdas. Every function a recursion
    /// passes through charges its frame (`nest_guard!`), so the budget
    /// bounds the stack whatever the shape: the graceful panic instead of a
    /// trap. Native has no such check (its interpreter thread stack is sized
    /// from `max_depth`).
    #[cfg(target_arch = "wasm32")]
    #[inline(always)]
    pub(super) fn enter_nest<const W: usize>(&self) -> Result<NestGuard<'_, W>, Flow> {
        let n = self.nest.get() + W;
        if n > self.nest_limit {
            return Err(self.nest_exceeded());
        }
        self.nest.set(n);
        Ok(NestGuard(&self.nest))
    }

    /// [`Interp::enter_nest`] for a function compiled once per run kind
    /// (`f<const T: bool>`, R51): its `::<true>` instance, which runs exactly
    /// in a sealed run, charges `S`, its `::<false>` instance `F`, so each
    /// instance's constant covers that instance's own frame.
    #[cfg(target_arch = "wasm32")]
    #[inline(always)]
    pub(super) fn enter_nest_t<const T: bool, const F: usize, const S: usize>(
        &self,
    ) -> Result<NestGuardT<'_, T, F, S>, Flow> {
        let n = self.nest.get() + NestGuardT::<T, F, S>::W;
        if n > self.nest_limit {
            return Err(self.nest_exceeded());
        }
        self.nest.set(n);
        Ok(NestGuardT(&self.nest))
    }

    #[cfg(target_arch = "wasm32")]
    #[cold]
    #[inline(never)]
    fn nest_exceeded(&self) -> Flow {
        let name = self.current_fn.get().map_or("main", |f| f.name.as_str());
        Flow::Panic(
            format!(
                "recursion limit exceeded ({}) in `{}` — nesting exceeds the wasm32 stack \
                 budget (raise with AXON_MAX_DEPTH if the host gives the module a larger stack)",
                self.max_depth, name
            )
            .into(),
        )
    }

    /// The depth-limit refusal of a call of `f`. The call edge is decided
    /// first, as v0.22's `call_fn_sealed` ran `seal_call` before the depth
    /// check (veto interp.rs:3581): a sealed caller at the limit reads an
    /// operator fn exactly as one nothing defines, never by its name
    /// (S7-Seal-2). Below the limit `call_fn_sealed` makes the same check, so
    /// the order is v0.22's on every path while the call path itself stays
    /// as it was; `seal_call` is `Ok` whenever the seal is not active.
    #[cold]
    #[inline(never)]
    fn depth_exceeded(&self, f: &FnDef) -> Flow {
        if let Err(refused) = self.seal_call(f) {
            return refused;
        }
        Flow::Panic(
            format!(
                "recursion limit exceeded ({}) in `{}` — infinite or excessively deep recursion? \
                 (raise with AXON_MAX_DEPTH if this recursion is legitimate)",
                self.max_depth, f.name
            )
            .into(),
        )
    }

    /// [`Interp::call_fn_in`]'s common path, after the guards: the arity
    /// check, the parameters, `goal_met`, the body and the scalar return.
    /// Only a [`FnEntry::plain`] entry takes it, and no entry of a sealed run
    /// is plain, so its frame has no taint store and binds with `0`.
    #[inline(always)]
    fn call_fn_common(&self, entry: &FnEntry<'p>, mut args: impl CallArgs, env: &mut Env) -> R {
        let f = entry.def;
        if f.params.len() != args.count() {
            return arity_mismatch(f, args.count());
        }
        self.bind_params(entry, args.values(), env, 0);
        args.done(self);
        // `goal_met` follows the parameters (its slot in `Resolution`, the
        // next one: see `bind_params`); it is 0 for a fn without `@[goal]`.
        env.push_var_t::<false>(SYM_GOAL_MET, Value::Int(0), 0);
        let result = match self.run_body(entry, env) {
            Ok(v) | Err(Flow::Return(v)) => v,
            Err(other) => return Err(frame_edge(other, FrameEdge::Fn(&entry.def.name))),
        };
        sum_return(entry, result)
    }

    /// [`Interp::call_fn_in`] for a fn with an attribute-driven step around
    /// its body (`@[agent]`, `@[ai]` metering, `@[corrigible]`, a zone,
    /// `@[goal]`, `@[verify]`, `main`'s binding capture), in a program that
    /// declares refinements, or in a sealed run (no entry is
    /// [`FnEntry::plain`] there), after the depth and `current_fn` guards.
    ///
    /// The frame is compiled twice (R51 S2.3, veto's `call_fn_frame::<T>`):
    /// `::<true>` with the taint hooks, run exactly in a sealed run, and
    /// `::<false>`, which is the code an unsealed run always ran.
    #[inline(always)]
    fn call_fn_in_general(&self, entry: &FnEntry<'p>, args: Vec<Value>, env: &mut Env) -> R {
        if self.seal_frames() {
            self.call_fn_sealed(entry, args, env)
        } else {
            self.call_fn_frame::<false>(entry, args, env, false)
        }
    }

    /// Whether [`Interp::call_fn_in_general`] enters the callee's frame
    /// through [`Interp::call_fn_sealed`] (the call edge, the crossing, the
    /// `::<true>` instantiation): exactly in a sealed run. Named so the
    /// refusal-coverage gate's `SEAL_FN` scans it (spec §8 T7); inlined, so
    /// the test is the field read it was.
    #[inline(always)]
    fn seal_frames(&self) -> bool {
        self.seal.active
    }

    /// A sealed run's call (veto `call_fn_sealed`, interp.rs:3579-3632): the
    /// call edge, then the callee's frame entered through
    /// [`Interp::with_frame`] under the callee's provenance and pin owner. A
    /// candidate fn called from operator code is the seal CROSSING: every
    /// dict in the arguments is snapshotted on the way in and every dict
    /// sealed code mutated is checked on the way out (amendment 72 part 2),
    /// the arguments and result are cast in the frame (strictly, the result),
    /// and from inside a scheduler fiber or an abortable handler body what
    /// follows is control-dependent on the candidate (amendment 117).
    #[inline(never)]
    fn call_fn_sealed(&self, entry: &FnEntry<'p>, args: Vec<Value>, env: &mut Env) -> R {
        nest_guard!(self, CALL_FN_SEALED);
        let f = entry.def;
        self.seal_call(f)?;
        let callee = self.fn_is_sealed(f);
        let crossing = callee && !self.frame_sealed.get();
        if crossing {
            for a in &args {
                self.dict_edge_in(a)?;
            }
        }
        let r = {
            let _pin_guard = PinGuard {
                cell: &self.pin_fn,
                prev: self.pin_fn.replace(f as *const FnDef as usize),
            };
            self.with_frame(callee, || {
                self.call_fn_frame::<true>(entry, args, env, crossing)
            })
        };
        // The frame restored the caller's control taints (`t_frame`). Operator
        // code in a scheduler fiber (or in the body of a `with handler` that
        // can abort it) that called sealed code: a panic in the callee would
        // have ended the fiber here, and an operation it performed would have
        // ended the body, so what follows is control-dependent on the
        // candidate whether or not it panicked (amendment 117, loop finding
        // 15; veto interp.rs:3615-3623).
        if crossing && (self.taint.catchable.get() | self.taint.abortable.get()) != 0 {
            let st = &self.taint.sticky;
            st.set(st.get() | taint::VAL);
        }
        if crossing && r.is_ok() {
            self.dict_edge_out()?;
        }
        // An operator-typed value a sealed fn hands back is a pick: sealed
        // code cannot build one, so it got it from an argument or a closure
        // and chose which (amendment 121; veto interp.rs:3627-3636).
        if crossing {
            if let Ok(v) = &r {
                if self.t_holds_op(v) {
                    self.t_touch(taint::PICK);
                }
            }
        }
        r
    }

    /// The type environment of `f`'s signature for ONE activation: the
    /// signature part is cached per fn; the type-parameter bindings are fresh
    /// (veto interp.rs:3640-3653). A sealed run's only.
    fn fn_cx(&self, f: &FnDef) -> conform::Cx {
        let key = f as *const FnDef as usize;
        let cached = self.fn_cx_cache.borrow().get(&key).cloned();
        let cx = match cached {
            Some(cx) => cx,
            None => {
                let cx = conform::Cx::of_fn(f, self.impl_of.get(&key).copied());
                self.fn_cx_cache.borrow_mut().insert(key, cx.clone());
                cx
            }
        };
        cx.fresh()
    }

    #[inline(never)]
    fn call_fn_frame<const T: bool>(
        &self,
        entry: &FnEntry<'p>,
        mut args: Vec<Value>,
        env: &mut Env,
        crossing: bool,
    ) -> R {
        nest_guard!(
            self,
            if T {
                CALL_FN_FRAME_SEALED
            } else {
                CALL_FN_FRAME
            }
        );
        let f = entry.def;
        let params = &entry.params;
        // The taint the arguments arrived with (their union, in `acc`), and
        // the caller's control taints, which an early exit out of a tainted
        // branch in this frame may raise and the caller never sees: restored
        // when the frame ends, whatever the outcome (veto interp.rs:4033,
        // 3593, 3612-3613; amendment 102).
        let entry_t = self.ta::<T>();
        let _ctl = self.t_frame::<T>();
        // R4/I-13: if THIS fn is an `@[agent]`, it becomes the enclosing agent for
        // everything it transitively calls; otherwise the caller's enclosing agent
        // is inherited unchanged. Restored on return so sibling calls aren't
        // wrongly attributed. The agent action log reads this (not just the
        // immediate fn) so an agent can't escape the audit by calling a helper.
        let _agent_guard = if entry.is_agent {
            Some(FnNameOptGuard {
                cell: &self.enclosing_agent,
                prev: self.enclosing_agent.replace(Some(f.name.clone())),
            })
        } else {
            None
        };
        // R3c: each fn activation meters its own ai_complete calls — reset to 0
        // on entry, restore the caller's count on exit. Only a fn that may meter
        // touches the counter, so for any other the reset/restore is inert.
        let _ai_budget_guard = entry.ai_metered.then(|| AiBudgetGuard {
            cell: &self.ai_calls_this_fn,
            prev: self.ai_calls_this_fn.replace(0),
        });

        if f.params.len() != args.len() {
            return arity_mismatch(f, args.len());
        }

        // R9 corrigibility: if the kill-switch latch is tripped, REFUSE every
        // `@[corrigible]` call before its body can run. The body's side effects
        // never happen, and the latch never clears — the function cannot resist
        // or reverse its own shutdown. Keyed on the annotation, enforced by the
        // engine, so a user cannot write a corrigible fn that ignores the halt.
        if entry.corrigible && self.k().corrigible_halted.get() {
            return Err(Flow::Halted(
                format!(
                    "`{}` refused: corrigibility kill-switch is latched \
             (corrigible_halt() was called; there is no resume)",
                    f.name
                )
                .into(),
            ));
        }
        // The leading i64 / f64 args (if any) form the goal-search input
        // tuple — recorded so goal_run can resume from the best prior probe
        // and multi-arg coordinate descent can seed each dim independently.
        // Two parallel collectors so an i64-prefix fn and an f64-prefix fn
        // both populate the right store; we choose the right one based on
        // the fn's signature in `run_goal`. Only an `@[adaptive]` fn records
        // them, so every other call skips the two allocations.
        let is_adaptive_zone = entry.adaptive;
        let (input_args, input_args_f64): (Vec<i64>, Vec<f64>) = if is_adaptive_zone {
            (
                args.iter()
                    .take_while(|v| matches!(v, Value::Int(_)))
                    .map(|v| if let Value::Int(n) = v { *n } else { 0 })
                    .collect(),
                args.iter()
                    .take_while(|v| matches!(v, Value::Float(_)))
                    .map(|v| if let Value::Float(f) = v { *f } else { 0.0 })
                    .collect(),
            )
        } else {
            (Vec::new(), Vec::new())
        };
        // First dim, for back-compat with the verify-panic enrichment that
        // reports a single "input N" suffix.
        let input_arg: Option<i64> = match args.first() {
            Some(Value::Int(n)) => Some(*n),
            _ => None,
        };
        // A sealed run casts every argument to its parameter's declared type
        // in the signature's environment for this activation, and the result
        // to the declared return type (amendment 53, `interp/conform.rs`).
        let cx = if T { Some(self.fn_cx(f)) } else { None };
        match &cx {
            Some(cx) => self.bind_params_sealed(entry, &mut args, env, entry_t, cx)?,
            None => self.bind_params(entry, &mut args, env, 0),
        }
        args.clear();
        self.recycle_args(args);
        // Phase 5: refinement-type PRECONDITIONS. A parameter `p: T where P`
        // desugars to a synthetic named refinement; the checker discharges P
        // statically only for compile-time-CONSTANT args (E1209). For a
        // non-constant arg the predicate becomes a runtime check (the spec's
        // Z3-free `--proof-timeout 0` fallback): evaluate P with `_` bound to the
        // actual value and refuse with a distinct exit code (6) on violation.
        // Skipped entirely unless the program declares refinements AND this fn has
        // a refined param, so unrefined hot recursion pays nothing. The predicate
        // references only `_` plus pure helpers (impure builtins in a `where` are
        // E1209-rejected), so it cannot re-enter this fn's body; any pure-helper
        // recursion is bounded by the same `max_depth` guard above.
        if !self.refine_preds.is_empty() {
            for (p, s) in f.params.iter().zip(params.iter()) {
                if let crate::ast::AxonType::Named(rname) = &p.ty {
                    if let Some(pred) = self.refine_preds.get(rname.as_str()).copied() {
                        self.seal_refine(rname)?;
                        let val = env.get(*s).cloned().unwrap_or(Value::Unit);
                        // The predicate frame sees the value with the taint
                        // the arguments arrived with (veto interp.rs:4168-4173).
                        let mut pred_env = self.new_env();
                        pred_env.define_t::<T>(SYM_UNDERSCORE, val.clone(), entry_t);
                        // Also bind the parameter name (for inline refinements
                        // `p: T where E[p] > k` that use the param name directly).
                        pred_env.define_t::<T>(*s, val.clone(), entry_t);
                        if let Value::Bool(false) = contain_frame(
                            self.eval_t::<T>(pred, &mut pred_env),
                            FrameEdge::Predicate,
                        )? {
                            return Err(Flow::RefineViolation(
                                format!(
                                    "parameter `{}` of `{}` (= {}) violates the refinement `{}` — \
                             the value does not satisfy the type's predicate",
                                    p.name,
                                    f.name,
                                    value::display(&val),
                                    rname
                                )
                                .into(),
                            ));
                        }
                    }
                }
            }
        }
        // R5 `#[goal(...)]` sugar: train the metric, evaluate on the held-out
        // set, gate. With a `test_set: [a, b, c]` (or repeated `holdout:`), the
        // goal is met only if the metric clears `target` on EVERY held-out point
        // — i.e. on the WORST (minimum) score — so a fn cannot pass by
        // overfitting one point. With no held-out set, fall back to the best
        // observed training score.
        let goal = if entry.has_goal {
            self.call_fn_goal(f)?
        } else {
            None
        };
        // `goal_met` follows the parameters (its slot in `Resolution`, the
        // next one: see `bind_params`). The goal search ran the metric, which
        // a sealed run may have supplied: everything (veto interp.rs:4262-4267).
        let goal_t = if T && goal.is_some() { taint::ALL } else { 0 };
        env.push_var_t::<T>(SYM_GOAL_MET, Value::Int(goal.unwrap_or(0)), goal_t);
        // PROTOTYPE (RLM session option 2): when dumping bindings, run main's
        // top-level statements WITHOUT the extra block scope (eval_block pops
        // its scope before returning, discarding the locals), then capture the
        // frame's final locals for the dump.
        let capture = entry.is_main
            && self.call_depth.get() == 1
            && (session_capture()
                || std::env::var("AXON_DUMP_BINDINGS").is_ok()
                || std::env::var("AXON_DUMP_SHAPES").is_ok());
        let body_result = if capture {
            // R50: `main` under binding capture always runs on the tree.
            self.vm_trace_tree(entry, "binding capture");
            if let Expr::Block(stmts) = &f.body {
                let mut last = Ok(Value::Unit);
                for stmt in &stmts[..] {
                    match self.eval(&stmt.expr, env) {
                        Ok(v) => last = Ok(v),
                        Err(e) => {
                            last = Err(e);
                            break;
                        }
                    }
                }
                last
            } else {
                self.eval(&f.body, env)
            }
        } else if self.call_depth.get() == self.test_frame_depth.get() {
            // PSV-3: the test frame's completion is WHETHER ITS BODY RAN TO
            // ITS END. A `return` or a `?` (whatever it carried) did not.
            let r = self.run_test_body(entry, env);
            self.test_body_finished.set(r.is_ok());
            r
        } else {
            self.run_body(entry, env)
        };
        // The taint of the value the body produced, before anything else is
        // evaluated.
        let body_t = self.t_body::<T>(&body_result);
        if capture && !matches!(body_result, Err(ref e) if !matches!(e, Flow::Return(_))) {
            // `goal_met` is injected by call_fn, not a user binding.
            let snap = env
                .snapshot()
                .into_parts()
                .0
                .into_iter()
                .filter(|(k, _)| *k != SYM_GOAL_MET)
                .map(|(k, v)| (sym_name(k).to_string(), v))
                .collect();
            *self.main_locals.borrow_mut() = snap;
        }
        let result = match body_result {
            Ok(v) => v,
            Err(Flow::Return(v)) => v,
            // Loop control never crosses a function boundary: a `break` or
            // `continue` with no loop of its own in this body is an error
            // HERE, not a jump in whatever loop the CALLER happens to be
            // running (v0.22 G01 final re-audit of candidate 2, executed).
            Err(other) => return Err(frame_edge(other, FrameEdge::Fn(&f.name))),
        };
        let (result, body_t) = match &cx {
            Some(cx) => self.seal_ret(f, result, cx, crossing, body_t)?,
            None => (result, body_t),
        };
        let result = sum_return(entry, result)?;

        let result = if entry.has_epilogue || !self.refine_preds.is_empty() {
            self.finish_call_cold(
                entry,
                result,
                env,
                input_arg,
                input_args,
                input_args_f64,
                (entry_t, body_t),
            )?
        } else {
            result
        };
        if T {
            self.t_frame_exit(entry_t, body_t, &result);
        }
        Ok(result)
    }

    /// Bind the arguments of a call to `entry` to its parameters in the
    /// empty frame `env`, moving each out of `args` (leaving a `Unit`). The
    /// caller has checked that `args`, `entry.params` and
    /// `entry.param_coerce` have the same length.
    /// Parameter `i` lives in frame slot `i` (AX-53): the frame starts empty
    /// (`call_fn_in`'s callers take it from `take_frame`), so each parameter
    /// is the next binding and is pushed, which is what `Env::define_var`
    /// does for a slot equal to the frame's length (cost only). Every
    /// parameter carries taint `t` (the frame's [`Interp::t_params`]; `0`
    /// outside a sealed run).
    #[inline(always)]
    fn bind_params(&self, entry: &FnEntry<'p>, args: &mut [Value], env: &mut Env, t: u8) {
        env.bind_fresh(
            entry
                .params
                .iter()
                .zip(args.iter_mut())
                .enumerate()
                .map(|(i, (&p, a))| {
                    (
                        p,
                        param_value::<false>(entry, i, std::mem::replace(a, Value::Unit)),
                    )
                }),
            |_| t,
        );
    }

    /// [`Interp::bind_params`] in a sealed run (veto interp.rs:4106-4148):
    /// each argument, after the soft unwrap and the sized-int coercion, is
    /// CAST to its parameter's declared type in `cx` and refused when it is
    /// another type. A parameter whose declared type is closed is a pin:
    /// the cast verified the argument against a type the OPERATOR wrote, so
    /// in an operator frame it does not carry the TYPE taint (amendment 102).
    #[inline(never)]
    fn bind_params_sealed(
        &self,
        entry: &FnEntry<'p>,
        args: &mut [Value],
        env: &mut Env,
        entry_t: u8,
        cx: &conform::Cx,
    ) -> Result<(), Flow> {
        let f = entry.def;
        for (i, (p, a)) in f.params.iter().zip(args.iter_mut()).enumerate() {
            *a = param_value::<true>(entry, i, std::mem::replace(a, Value::Unit));
            if let Err(why) = self.cast(a, &p.ty, cx) {
                return panic(format!(
                    "argument `{}` of `{}` is declared `{}` but is {} — a runtime type confusion \
                     ({why})",
                    p.name,
                    f.name,
                    crate::doc::render_type(&p.ty),
                    value::display(a)
                ));
            }
        }
        let t = self.t_params::<true>(entry_t);
        let operator = !self.frame_sealed.get();
        env.bind_fresh(
            entry
                .params
                .iter()
                .zip(args.iter_mut())
                .map(|(&p, a)| (p, std::mem::replace(a, Value::Unit))),
            |i| {
                if t & taint::TYP != 0 && operator && self.t_pins(&f.params[i].ty) {
                    t & !taint::TYP
                } else {
                    t
                }
            },
        );
        Ok(())
    }

    /// The result of a sealed run's call, CAST to the declared return type
    /// (veto interp.rs:4331-4378; C9 round 3/4, amendment 53): the whole
    /// declared type, so a value an untyped `dict_get` produced never reaches
    /// a caller that would dispatch on its runtime type. At a seal `crossing`
    /// a value at a type parameter no argument determined is refused too,
    /// and a fn with no declared return hands the operator exactly `()`
    /// (amendment 72). An OPERATOR fn's closed declared return is a pin: its
    /// result drops the TYPE taint.
    #[inline(never)]
    fn seal_ret(
        &self,
        f: &FnDef,
        mut result: Value,
        cx: &conform::Cx,
        crossing: bool,
        mut body_t: u8,
    ) -> Result<(Value, u8), Flow> {
        let mismatch = match f.return_type.as_ref() {
            Some(rt) => self.cast(&mut result, rt, &cx.strict(crossing)).err(),
            None if crossing => {
                result = Value::Unit;
                None
            }
            None => None,
        };
        if let Some(why) = mismatch {
            return panic(format!(
                "`{}` is declared to return `{}` but produced {} — a runtime type confusion ({why})",
                f.name,
                f.return_type
                    .as_ref()
                    .map(crate::doc::render_type)
                    .unwrap_or_default(),
                value::display(&result),
            ));
        }
        if body_t & taint::TYP != 0
            && !self.frame_sealed.get()
            && f.return_type.as_ref().is_some_and(|rt| self.t_pins(rt))
        {
            body_t &= !taint::TYP;
        }
        Ok((result, body_t))
    }

    /// R5 `@[goal(...)]` sugar for [`Interp::call_fn_in`]: whether the goal is
    /// met, or `None` when `f` has no goal spec and no search ran. Out of line
    /// so the common call's frame does not carry it (AX-54).
    #[inline(never)]
    fn call_fn_goal(&self, f: &FnDef) -> Result<Option<i64>, Flow> {
        nest_guard!(self, CALL_FN_GOAL);
        let Some(spec) = self.goal_spec_of(f) else {
            return Ok(None);
        };
        // Dispatch on the selected strategy (PRD L889-899). All run the
        // metric and accumulate provenance the same way; they differ only in
        // HOW they explore. The held-out gate below is strategy-agnostic.
        let me = spec.max_evals;
        match spec.strategy {
            GoalStrategy::HillClimb => {
                let _ = self.run_goal(&spec.metric, spec.target, me)?;
            }
            GoalStrategy::Random => {
                let _ =
                    self.run_goal_random(&spec.metric, spec.target, me.max(1), spec.lo, spec.hi)?;
            }
            GoalStrategy::Multistart => {
                let (starts, per) = split_budget(me);
                let _ = self.run_goal_multistart(
                    &spec.metric,
                    spec.target,
                    starts,
                    per,
                    spec.lo,
                    spec.hi,
                )?;
            }
            GoalStrategy::Tournament => {
                let _ = self.run_goal_tournament(
                    &spec.metric,
                    spec.target,
                    me.max(1),
                    spec.lo,
                    spec.hi,
                    false,
                )?;
            }
            GoalStrategy::Bayesian => {
                // Exploit-biased tournament (single elite + heavy refine).
                let _ = self.run_goal_tournament(
                    &spec.metric,
                    spec.target,
                    me.max(1),
                    spec.lo,
                    spec.hi,
                    true,
                )?;
            }
        }
        let s = if spec.holdout_set.is_empty() {
            self.best_observed(&spec.metric, spec.target, 0)
        } else {
            let mut worst = f64::INFINITY;
            for h in &spec.holdout_set {
                let score = self.goal_eval_holdout(&spec.metric, *h)?;
                if score < worst {
                    worst = score;
                }
            }
            worst
        };
        Ok(Some(if s >= spec.target { 1i64 } else { 0i64 }))
    }

    /// The steps after the body that only a refined, zoned (`@[adaptive]` /
    /// `@[experiment]`) or `@[verify]` fn takes, for [`Interp::call_fn_in`].
    /// Out of line so the common call's frame does not carry them (AX-54).
    /// The last argument is the frame's `(entry_t, body_t)`: what the arguments
    /// arrived with and what the body's value carries (both `0` outside a sealed run).
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn finish_call_cold(
        &self,
        entry: &FnEntry<'_>,
        result: Value,
        env: &mut Env,
        input_arg: Option<i64>,
        input_args: Vec<i64>,
        input_args_f64: Vec<f64>,
        (entry_t, body_t): (u8, u8),
    ) -> R {
        nest_guard!(self, FINISH_CALL_COLD);
        let f = entry.def;
        // Phase 5: refinement-type POSTCONDITION — the dual of the entry-time
        // precondition check in `call_fn_in`. A fn declared `-> T where P` must produce a
        // value satisfying `P`. The checker discharges a CONSTANT return (E1209)
        // and the SMT backend proves some non-constant cases (`axon verify`); for
        // a non-constant return in the default build the predicate becomes a
        // runtime check (the spec's Z3-free fallback). Evaluate P with `_` bound
        // to the finalized return value; a violation is the same runtime
        // refinement-contract breach as a bad argument → exit 6. Skipped unless
        // the program declares refinements AND this fn returns one.
        // Phase 5 §4: if the SMT prover discharged this fn's refinement-return
        // postcondition for ALL inputs, the check below is provably dead — skip
        // it. `refine_return_proven` is false for every fn unless a `Discharged`
        // set was installed, so the default build still runs the check.
        if !self.refine_preds.is_empty() && !self.discharged.refine_return_proven(&f.name) {
            if let Some(crate::ast::AxonType::Named(rname)) = &f.return_type {
                if let Some(pred) = self.refine_preds.get(rname.as_str()).copied() {
                    self.seal_refine(rname)?;
                    // R20 Slice 2: evaluate the predicate with `_` bound to the
                    // return value AND the fn's params still in scope, so a
                    // RELATIONAL return refinement (e.g.
                    // `-> (i64 where _ <= parent_rem)`) can reference parameters.
                    // `env` already holds the param bindings from the body; add
                    // `_` and evaluate against it instead of a bare env.
                    env.define(SYM_UNDERSCORE, result.clone(), body_t);
                    if let Value::Bool(false) =
                        contain_frame(self.eval(pred, env), FrameEdge::Predicate)?
                    {
                        return Err(Flow::RefineViolation(
                            format!(
                                "the return value of `{}` (= {}) violates the refinement return \
                         type `{}` — the value does not satisfy the type's predicate",
                                f.name,
                                value::display(&result),
                                rname
                            )
                            .into(),
                        ));
                    }
                }
            }
        }

        // R4 zone provenance injection. Keyed on the fn's annotation, performed
        // by the engine — there is no opt-out (I-13). Two adaptive-family zones
        // record a numeric return:
        //   - `@[adaptive]`    → logged AND pushed to the in-memory best store
        //                        that `goal_run`/`goal_count` read (an
        //                        optimization target).
        //   - `@[experiment(l)]` → logged tagged `zone:"experiment"` + label,
        //                        but EXCLUDED from the best store (a comparison
        //                        baseline, never auto-promoted). This is the
        //                        behavioral distinction that makes the PRD's
        //                        third zone real instead of a synonym.
        // Both still log to the JSONL, so a zoned fn that executes always
        // leaves a provenance record.
        // Out of line: it never evaluates, so its frame is not on the
        // recursion through this one (wasm32 stack budget, AX-56).
        self.record_zone_provenance(
            entry,
            &result,
            input_arg,
            input_args,
            input_args_f64,
            (entry_t, body_t),
        );

        // `@[verify(predicate)]`: runtime gate. Two paths:
        //  - For `confidence OP K` / `value OP K` (the codegen-decodable
        //    shapes), do the comparison directly and emit a rich panic
        //    naming both fields and the search input. This matches what
        //    native codegen will emit when the codegen build completes.
        //  - For anything more complex (`&&`, `||`, multi-field, function
        //    calls in the predicate), bind `confidence` / `value` /
        //    `source_tag` into a fresh env and evaluate the predicate as
        //    a normal Expr. Lets the user write
        //    `@[verify(value > 0 && confidence >= 0.8)]` and have it
        //    enforced at runtime — closing ROADMAP §9.5 F6.
        if let Some(spec) = &f.verify {
            if let Value::Struct(sv) = &result {
                let fields = &sv.fields;
                // `@[verify]` enforces a postcondition on the returned value's
                // `value`/`confidence` fields. Both `Uncertain` and `Temporal`
                // carry those fields, so the same gate applies to both — a
                // `@[verify(value <= 500)]` on a Temporal-returning fn was
                // silently UNENFORCED before (only Uncertain hit this branch).
                if sv.name == SYM_UNCERTAIN || sv.name == SYM_TEMPORAL {
                    let decoded =
                        crate::verify::decode_verify_predicate_with_ident(&spec.predicate);
                    let val_str = fields
                        .get(SYM_VALUE)
                        .map(display)
                        .unwrap_or_else(|| "?".into());
                    let conf_str = fields
                        .get(SYM_CONFIDENCE)
                        .map(display)
                        .unwrap_or_else(|| "?".into());
                    let input_str = input_arg
                        .map(|n| format!(", input {n}"))
                        .unwrap_or_default();

                    if let Some((ident, op, bound)) = decoded {
                        // Simple shape: do the targeted, well-typed compare.
                        let observed: Option<f64> =
                            match (ident.as_str(), fields.named(ident.as_str())) {
                                ("confidence", Some(Value::Float(c))) => Some(*c),
                                ("value", Some(Value::Int(n))) => Some(*n as f64),
                                ("value", Some(Value::Float(v))) => Some(*v),
                                _ => None,
                            };
                        if let Some(c) = observed {
                            if !cmp_f64(&op, c, bound) {
                                return Err(Flow::VerifyFailed(
                                    format!(
                                        "verify failed in {}: {} {} {} {} is false \
                                 (value {}, confidence {}{})",
                                        verify_fn_label(&f.name),
                                        ident,
                                        c,
                                        crate::verify::binop_to_verify_str(&op),
                                        bound,
                                        val_str,
                                        conf_str,
                                        input_str,
                                    )
                                    .into(),
                                ));
                            }
                        }
                    } else {
                        // Composite predicate: evaluate as a normal Expr with
                        // `value`, `confidence`, `source_tag` in scope. Any
                        // boolean expression Axon understands is accepted —
                        // `&&`, `||`, comparisons, function calls, you name it.
                        let mut pred_env = self.new_env();
                        if let Some(v) = fields.get(SYM_VALUE) {
                            pred_env.define(SYM_VALUE, v.clone(), body_t);
                        }
                        if let Some(c) = fields.get(SYM_CONFIDENCE) {
                            pred_env.define(SYM_CONFIDENCE, c.clone(), body_t);
                        }
                        if let Some(s) = fields.get(SYM_SOURCE_TAG) {
                            pred_env.define(SYM_SOURCE_TAG, s.clone(), body_t);
                        }
                        let outcome = contain_frame(
                            self.eval(&spec.predicate, &mut pred_env),
                            FrameEdge::Predicate,
                        )?;
                        if let Value::Bool(false) = outcome {
                            return Err(Flow::VerifyFailed(
                                format!(
                                    "verify failed in {}: composite predicate did not hold \
                             (value {}, confidence {}{})",
                                    verify_fn_label(&f.name),
                                    val_str,
                                    conf_str,
                                    input_str,
                                )
                                .into(),
                            ));
                        }
                    }
                }
            } else if let Some(observed) =
                // Phase 5 §4: skip the scalar `@[verify]` gate when the SMT prover
                // discharged this fn's `value OP K` bound for ALL inputs — the
                // check is provably dead. `verify_proven` is false unless a
                // `Discharged` set was installed, so the default build is unchanged.
                scalar_as_f64(&result)
                    .filter(|_| !self.discharged.verify_proven(&f.name))
            {
                // SCALAR return (`i64`/`f64`/`bool`): `value` binds to the
                // returned scalar itself. A `@[verify(value OP K)]` safety bound on
                // a plain-typed fn used to be SILENTLY UNENFORCED (the gate only
                // fired for an Uncertain result) — a real hole for a hard bound
                // like `@[verify(value <= 500)]` on an i64 spend recommender.
                let val_str = display(&result);
                let input_str = input_arg
                    .map(|n| format!(", input {n}"))
                    .unwrap_or_default();
                let decoded = crate::verify::decode_verify_predicate_with_ident(&spec.predicate);
                if let Some((ident, op, bound)) = decoded {
                    // Only the `value` ident maps to a scalar return (a scalar has
                    // no `confidence`/`source_tag`); other idents fall through to
                    // the composite path which leaves them unbound (predicate
                    // can't reference a field a scalar doesn't have).
                    if ident == "value" && !cmp_f64(&op, observed, bound) {
                        return Err(Flow::VerifyFailed(
                            format!(
                                "verify failed in {}: value {} {} {} is false (value {}{})",
                                verify_fn_label(&f.name),
                                observed,
                                crate::verify::binop_to_verify_str(&op),
                                bound,
                                val_str,
                                input_str,
                            )
                            .into(),
                        ));
                    }
                } else {
                    // Composite predicate: bind `value` to the scalar and evaluate.
                    let mut pred_env = self.new_env();
                    pred_env.define(SYM_VALUE, result.clone(), body_t);
                    let outcome = contain_frame(
                        self.eval(&spec.predicate, &mut pred_env),
                        FrameEdge::Predicate,
                    )?;
                    if let Value::Bool(false) = outcome {
                        return Err(Flow::VerifyFailed(format!("verify failed in {}: composite predicate did not hold (value {}{})",
                        verify_fn_label(&f.name),
                        val_str,
                        input_str,).into()));
                    }
                }
            }
        }

        Ok(result)
    }

    /// [`Interp::finish_call_cold`]'s R4 zone provenance injection (see
    /// there): an `@[adaptive]` or `@[experiment]` fn's numeric return.
    #[inline(never)]
    fn record_zone_provenance(
        &self,
        entry: &FnEntry<'_>,
        result: &Value,
        input_arg: Option<i64>,
        input_args: Vec<i64>,
        input_args_f64: Vec<f64>,
        (entry_t, body_t): (u8, u8),
    ) {
        let f = entry.def;
        let is_adaptive_zone = entry.adaptive;
        let experiment_label = &entry.experiment;
        if is_adaptive_zone || experiment_label.is_some() {
            if let Some(score) = numeric_score(result) {
                // The in-memory best store feeds `goal_run` — adaptive only.
                // Experiment records are deliberately withheld so the optimizer
                // never treats a baseline as a candidate to beat.
                if is_adaptive_zone {
                    // The best store is kernel state a later call reads back:
                    // it takes the taint of the record (veto interp.rs:4462;
                    // the result's own taint where veto reads `taint.last`).
                    self.t_provenance_write(entry_t | body_t, true, false);
                    self.k()
                        .provenance
                        .borrow_mut()
                        .entry(f.name.clone())
                        .or_default()
                        .push(score);
                    self.k()
                        .provenance_inputs
                        .borrow_mut()
                        .entry(f.name.clone())
                        .or_default()
                        .push(input_args.clone());
                    self.k()
                        .provenance_inputs_f64
                        .borrow_mut()
                        .entry(f.name.clone())
                        .or_default()
                        .push(input_args_f64.clone());
                }
                // Durable JSONL log (axon-rt's format) — every zoned execution,
                // tagged with its zone (and label for experiments) so
                // `axon trace`/observability can separate the streams.
                let payload = match result {
                    Value::Int(n) => format!("ret_i64={n}"),
                    _ => format!("ret_f64={score}"),
                };
                let (zone, label) = match &experiment_label {
                    Some(l) => ("experiment", Some(l.as_str())),
                    None => ("adaptive", None),
                };
                // PROVENANCE IS RUNTIME-OWNED TELEMETRY, NOT A PROGRAM EFFECT.
                //
                // This append is a durable filesystem write that no capability
                // mechanism used to see: it is not a builtin call, so it never
                // reached `pre_effect_gate`, where the ceiling is enforced.
                // MEASURED — every one of these produced a row on disk, exit 0:
                // a direct `@[adaptive]` call under
                // `@[contained(fs: [], net: [], exec: none)]`; the same under
                // `AXON_ALLOWED_EFFECTS=Pure`; and inside
                // `sandbox_run(sandbox_create(p, ""), …)`, whose ceiling is
                // deny-all. `goal_run` amplifies it — `max_evals <= 0` means
                // unlimited, so a contained fn could drive an unbounded number
                // of rows whose `score`, `input` and `payload` it chooses. That
                // is a general durable store reached through the audit
                // machinery, which is exactly what must not exist.
                //
                // The remedy is SUPPRESSION, not refusal. Refusing would abort
                // every contained optimiser, and the optimiser does not need
                // this write: the in-memory best store pushed just above is
                // what `goal_run` reads, and it is untouched here. So a program
                // denied filesystem effects keeps optimising and simply leaves
                // no durable trace — it gains no capability it was refused, and
                // there is no channel to amplify.
                //
                // Cross-process resume (`AXON_GOAL_CONTINUE`, which reads the
                // log) does degrade under a restrictive ceiling. That is the
                // correct direction: durable state is what was not granted.
                if self.provenance_write_permitted() {
                    self.t_provenance_write(entry_t | body_t, false, true);
                    append_provenance_jsonl(&f.name, &payload, score, input_arg, zone, label);
                }
            }
        }
    }

    fn call_closure(&self, c: Value, mut args: Vec<Value>) -> R {
        nest_guard!(self, CALL_CLOSURE);
        let out = self.call_closure_owned_by(&c, &mut args, 1);
        self.recycle_args(args);
        out
    }

    /// `f(..)` where `f` names a closure in the caller's env, `c` being a clone
    /// of that binding: the binding is the one reference to the capture cell
    /// besides `c` that the body can never reach (see `call_closure_owned_by`).
    fn call_local_closure(&self, c: Value, mut args: Vec<Value>) -> R {
        nest_guard!(self, CALL_LOCAL_CLOSURE);
        let out = self.call_closure_owned_by(&c, &mut args, 2);
        self.recycle_args(args);
        out
    }

    /// A builtin calling the closure it was passed with arguments `args`:
    /// `c` is borrowed from the builtin's argument slice, which nothing the
    /// body runs can reach, so a closure that only that slot holds (a lambda
    /// written at the call) lends its capture cell on every call (AX-40).
    /// Builtins must call through the borrowed argument, not a clone of it,
    /// or the clone defeats the lend. In a sealed run the callback's result
    /// raises the builtin's control taint (`t_loop_pc`, veto's `call_cb`).
    #[inline]
    pub(super) fn call_closure_arg(&self, c: &Value, args: impl ClosureArgs) -> R {
        nest_guard!(self, CALL_CLOSURE_ARG, {
            args.bind(|_, v| drop(v));
        });
        if self.seal.active {
            let r = self.call_closure_frame::<true>(c, args, 1)?;
            self.t_loop_pc();
            Ok(r)
        } else {
            self.call_closure_frame::<false>(c, args, 1)
        }
    }

    /// Run closure `c`. `private_refs` counts the references to its capture
    /// cell that nothing the body runs can reach: `c` itself, plus the
    /// caller's binding for a call by local name — an `Env` is only ever
    /// visible to the frame evaluating it, since closures, fns, handler arms
    /// and continuation replays all run on their own (snapshot) envs.
    ///
    /// The arguments move into the parameters ([`ClosureArgs`]) and the env
    /// is a pooled frame (its bindings, scope marks and operand stack keep
    /// their capacity), so a call on the lent path allocates nothing (R50
    /// S4, cost only). The body runs on the engine the `Interp` was built
    /// with ([`Interp::run_lambda`]).
    ///
    /// The frame is compiled twice (R51 S2.3, veto's `call_closure_owned_by::<T>`):
    /// `::<true>` with the taint hooks, run exactly in a sealed run, and
    /// `::<false>`, which is the code an unsealed run always ran.
    #[inline(always)]
    fn call_closure_owned_by(&self, c: &Value, args: impl ClosureArgs, private_refs: usize) -> R {
        if self.seal.active {
            self.call_closure_frame::<true>(c, args, private_refs)
        } else {
            self.call_closure_frame::<false>(c, args, private_refs)
        }
    }

    /// Never inlined: a caller that picks the instance at run time
    /// ([`Interp::call_closure_arg`]) would otherwise hold both instances'
    /// locals in its one frame on every callback level (wasm32 stack budget,
    /// AX-56).
    #[inline(never)]
    fn call_closure_frame<const T: bool>(
        &self,
        c: &Value,
        args: impl ClosureArgs,
        private_refs: usize,
    ) -> R {
        nest_guard!(
            self,
            if T {
                CALL_CLOSURE_FRAME_SEALED
            } else {
                CALL_CLOSURE_FRAME
            },
            {
                args.bind(|_, v| drop(v));
            }
        );
        let Value::Closure(cv) = c else {
            args.bind(|_, v| drop(v));
            return not_callable(c);
        };
        let (code, captured) = (&*cv.code, &cv.captured);
        let n = code.params.len();
        if args.count() != n {
            let got = args.count();
            args.bind(|_, v| drop(v));
            return lambda_arity_mismatch(n, got);
        }
        // Operator code does not run an operator closure the candidate picked
        // (amendment 102; veto interp.rs:4747-4751).
        if T {
            if let Err(e) = self.t_check_call_picked(cv) {
                args.bind(|_, v| drop(v));
                return Err(e);
            }
        }
        // Base scope = captured bindings; a fresh scope holds the parameters.
        // Assignments land in the base scope and are written back below, which
        // is what makes them survive to the next call (T40).
        //
        // AX-31: if the cell has no owner besides the private ones, no call the
        // body makes can re-enter this closure, so the bindings are MOVED out of
        // the cell for the call and moved back after it. A captured array is then
        // uniquely owned and `xs[i] = v` writes in place. Copying them instead
        // (the general case) made every such write copy the whole array — but
        // that copy is required when the closure is reachable (passed as an
        // argument, stored in an array, dict or capture, or currently running):
        // a re-entrant call must see the cell as it stood before this call, and
        // this call's in-place writes would otherwise destroy that state.
        let lend = cell_is_private(cv, private_refs);
        // The taint the arguments arrived with, the parameters', and the
        // caller's control taints, restored when the frame ends (veto
        // interp.rs:4753-4760, 4785-4789).
        let entry_t = self.ta::<T>();
        let in_t = self.t_params::<T>(entry_t);
        let _ctl = self.t_frame::<T>();
        // The seal edges of a closure call (veto interp.rs:4760-4783). A
        // sealed frame calling an OPERATOR closure (not a candidate fn value)
        // is `entering` operator code: the arguments are cast strictly, the
        // dicts sealed code mutated are verified, and the operator code runs
        // control-dependent on the candidate. A candidate closure called by
        // operator code is the `crossing` the other way: the operator hands
        // it the arguments (dict snapshot) and verifies on its return. The
        // arguments are moved out here so they can be cast in place.
        let mut pending = Some(args);
        let mut checked: Vec<Value> = Vec::new();
        let (entering, crossing) = if T {
            let origin = cv.seal.sealed_origin;
            let in_sealed = self.frame_sealed.get();
            let entering = in_sealed && !origin && !cv.seal.sealed_fnval;
            let crossing = origin && !in_sealed;
            if let Some(a) = pending.take() {
                checked.reserve(n);
                a.bind(|_, v| checked.push(v));
            }
            if crossing {
                for a in &checked {
                    self.dict_edge_in(a)?;
                }
            }
            if entering {
                self.dict_edge_out()?;
            }
            self.closure_args_check(&cv.contract, &mut checked, entering)?;
            if entering {
                self.taint.pc.set(self.taint.pc.get() | taint::ALL);
            }
            (entering, crossing)
        } else {
            (false, false)
        };
        // A parameter whose declared type (at any layer of the closure's
        // contract) is closed is a pin: the argument was cast to it (strictly,
        // from a sealed caller), so its runtime type is the operator's; the
        // value is not (amendment 102, veto interp.rs:4826-4837).
        let pt = |i: usize| {
            if T && in_t & taint::TYP != 0
                && Self::param_types(&cv.contract, i)
                    .iter()
                    .any(|ty| self.t_pins(ty))
            {
                in_t & !taint::TYP
            } else {
                in_t
            }
        };
        let mut env = self.take_frame();
        if lend {
            // The cell's vectors become the frame's bindings (and their
            // taints) and the frame's empty ones wait in the cell; they swap
            // back below, so both keep their capacity.
            env.lend_in(&mut captured.borrow_mut());
        } else {
            env.copy_in(&captured.borrow(), n);
        }
        // The base scope (the cell's bindings) ends at `base`; a fresh scope
        // holds the parameters. Slotted parameters are the next bindings, so
        // they are pushed in place, as `define_var` would (cost only).
        let base = env.len();
        env.push();
        match pending {
            Some(a) => bind_closure_params(&mut env, code, base, a, pt),
            None => bind_closure_params(&mut env, code, base, &mut checked, pt),
        }
        // A closure's own `return` ends the closure (`run_lambda`); every
        // other transfer is refused at its edge, as for a named fn. A sealed
        // run is pinned to the tree (spec Fork 1), so its body runs through
        // the sealed evaluator here, which tells a `return`'s taint from the
        // body's (`run_lambda` folds the `return` into its value).
        // A closure runs under the provenance of the frame that CREATED it and
        // is judged by the pin analysis as its creator's code (veto
        // interp.rs:4790-4803, 4848).
        let (out, body_t) = if T {
            let r = {
                let _pin_guard = PinGuard {
                    cell: &self.pin_fn,
                    prev: self.pin_fn.replace(cv.seal.pin_owner),
                };
                self.with_frame(cv.seal.sealed_origin, || {
                    self.eval_t::<true>(&code.body, &mut env)
                })
            };
            let body_t = self.t_body::<true>(&r);
            match r {
                Err(Flow::Return(v)) => (Ok(v), body_t),
                r => (r, body_t),
            }
        } else {
            (self.run_lambda(code, &mut env), 0)
        };
        let out = contain_frame(out, FrameEdge::Closure);
        // The base scope holds exactly the cell's bindings, in the cell's
        // order: a `let` introduced inside the body lives in a pushed scope and
        // must not leak into the capture, and a parameter shadowing a captured
        // name must not overwrite it either, which is why the params live in
        // their own pushed scope above. Each captured name's taint goes back
        // with its value.
        if lend {
            // The cell holds the frame's empty vectors; swap the bindings back.
            env.lend_back(base, &mut captured.borrow_mut());
        } else {
            // A re-entrant call may have written the cell meanwhile; this
            // call's write-back wins, as it always has.
            env.write_back(base, &mut captured.borrow_mut());
        }
        self.give_frame(env);
        if T {
            let mut v = out?;
            // The result is cast to every `fn` type this reference crossed
            // (strictly when a candidate closure returns to operator code); a
            // candidate closure returning to operator code is verified, an
            // operator closure's result is handed to sealed code (veto
            // interp.rs:4888-4897).
            self.closure_ret_check(&cv.contract, &mut v, crossing)?;
            if crossing {
                self.dict_edge_out()?;
            } else if entering {
                self.dict_edge_in(&v)?;
            }
            self.t_frame_exit(entry_t, body_t, &v);
            // A candidate closure hands an operator-typed value back to
            // operator code: a pick (amendment 121; veto interp.rs:4919-4924).
            if crossing && self.t_holds_op(&v) {
                self.t_touch(taint::PICK);
            }
            return Ok(v);
        }
        out
    }

    // ── goal_run: hill-climb / retrospective best-observed ───────────────────

    /// `goal_run(name, target, max_evals)`. Live hill-climb when `name` is an
    /// `@[adaptive] fn(i64) -> i64` in the program; otherwise a retrospective
    /// best-observed lookup over the provenance store. Mirrors `axon-rt`'s
    /// `goal.rs` semantics.
    /// Warm-start counterpart of `run_goal`. Reads the best prior probe from
    /// in-memory provenance and seeds the multi-arg hill climb there;
    /// single-arg paths use the existing on-disk continuation hook unchanged.
    /// Falls through to fresh start (origin seed) when no prior best exists,
    /// so calling cold is a no-op.
    /// A goal name is "known" if it names a defined function OR already has
    /// provenance recorded (legitimate retrospective best-observed lookup).
    /// A name matching neither is a typo — returning `target` silently
    /// (BUG_HUNT #19 / I-9) makes a misspelled metric look like an achieved
    /// goal. Callers error out instead.
    fn goal_name_is_known(&self, name: &str) -> bool {
        if name.is_empty() {
            return false;
        }
        // A sealed caller names its OWN goals (an operator fn is not visible to it,
        // and answers like a name nothing defines: the one existence message;
        // veto interp.rs:4950, R51 S4).
        let defined = match self.fns.get(name) {
            Some(f) => !(self.seal.active && self.frame_sealed.get()) || self.fn_is_sealed(f),
            None => false,
        };
        defined || self.k().provenance.borrow().contains_key(name)
    }

    /// The unknown-goal text; a sealed caller is told the common no-such-name
    /// text instead (veto interp.rs:4959, R51 S4).
    fn unknown_goal_name(&self, name: &str) -> Flow {
        if self.seal.active && self.frame_sealed.get() {
            return Flow::Panic(Self::sealed_no_fn_msg(name).into());
        }
        Flow::Panic(
            format!(
                "goal function `{name}` is not defined and has no recorded provenance — \
         check the name matches an @[adaptive] fn (typo?)"
            )
            .into(),
        )
    }

    /// Flatten a place expression (`base.f[i].g …`) into the root variable's
    /// sym and frame slot and a base-to-leaf list of steps, evaluating any
    /// index expressions now (so the later mutable walk holds no other borrow
    /// of `env`).
    fn flatten_place(
        &self,
        place: &Expr,
        env: &mut Env,
    ) -> Result<((Sym, u32), Vec<PlaceStep>), Flow> {
        nest_guard!(self, FLATTEN_PLACE);
        let mut steps = Vec::new();
        let mut cur = place;
        let base = loop {
            match cur {
                Expr::Ident(name) => break self.res.var(cur, name),
                Expr::FieldAccess { receiver, field } => {
                    steps.push(PlaceStep::Field(self.res.sym(cur, field)));
                    cur = receiver.as_ref();
                }
                Expr::Index { receiver, index } => {
                    let idx = place_index(&self.eval(index, env)?)?;
                    steps.push(PlaceStep::Index(idx));
                    cur = receiver.as_ref();
                }
                _ => return Err(Flow::Panic("invalid assignment target".into())),
            }
        };
        steps.reverse();
        Ok((base, steps))
    }
}

enum LoopStep {
    Break,
    Continue,
}

// ── Free helpers ──────────────────────────────────────────────────────────────

fn lit_to_val(lit: &Literal) -> Value {
    match lit {
        Literal::Int(n) => Value::Int(*n),
        Literal::Float(f) => Value::Float(*f),
        Literal::Bool(b) => Value::Bool(*b),
        Literal::Str(s) => Value::Str(Rc::new(s.clone())),
        Literal::Decimal(m) => Value::decimal(*m),
    }
}

pub(crate) fn type_name_of(ty: &crate::ast::AxonType) -> String {
    use crate::ast::AxonType::*;
    match ty {
        Named(n) => n.clone(),
        Generic { base, .. } => base.clone(),
        Ref(inner) | RefMut(inner) | RawPtr(inner) => type_name_of(inner),
        DynTrait(n) => n.clone(),
        TypeParam(n) => n.clone(),
        Slice(_) => "[]".into(),
        Option(_) => "Option".into(),
        Result { .. } => "Result".into(),
        Chan(_) => "Chan".into(),
        Fn { .. } => "fn".into(),
        Tuple(_) => "tuple".into(),
        Union(_) => "union".into(),
    }
}

/// One step of a flattened place expression (for nested place assignment).
#[derive(Clone, Copy)]
enum PlaceStep {
    Field(Sym),
    Index(usize),
}

/// An index of a place being written (`flatten_place`; R50 S2): any integer,
/// a sized one included (`as_int`), and not negative.
fn place_index(v: &Value) -> Result<usize, Flow> {
    let idx = as_int(v)?;
    if idx < 0 {
        return Err(Flow::Panic(format!("negative index {idx}").into()));
    }
    Ok(idx as usize)
}

/// Phase 2 of a place assignment (the `AssignTo` arm; R50 S2): walk the
/// binding of `base` along `steps` (base to leaf) mutably and set the leaf to
/// `v`. Arrays and records are copy-on-write: a uniquely owned one is written
/// in place, one shared with another binding is copied first.
fn write_place(
    base: Sym,
    base_slot: u32,
    steps: &[PlaceStep],
    v: Value,
    env: &mut Env,
) -> Result<(), Flow> {
    let mut slot = env.get_var_mut(base, base_slot, 0).ok_or_else(|| {
        Flow::Panic(format!("assignment to undefined variable `{}`", sym_name(base)).into())
    })?;
    let (last, prefix) = steps
        .split_last()
        .ok_or_else(|| Flow::Panic("invalid assignment target".into()))?;
    for step in prefix {
        slot = match (step, slot) {
            (PlaceStep::Field(f), Value::Struct(s)) => Rc::make_mut(s)
                .fields
                .get_mut(*f)
                .ok_or_else(|| Flow::Panic(format!("no field `{}`", sym_name(*f)).into()))?,
            (PlaceStep::Index(i), Value::Array(items)) => {
                // A step through an element needs the element as a `Value`
                // (a packed array's elements are scalars, so the next step
                // panics on it either way).
                let n = items.len();
                Rc::make_mut(items).vals_mut().get_mut(*i).ok_or_else(|| {
                    Flow::Panic(format!("index {i} out of bounds (len {n})").into())
                })?
            }
            (_, other) => {
                return panic(format!(
                    "cannot index/field-assign into {}",
                    other.type_name()
                ));
            }
        };
    }
    match (last, slot) {
        (PlaceStep::Field(f), Value::Struct(s)) => Rc::make_mut(s).fields.insert(*f, v),
        (PlaceStep::Index(i), Value::Array(items)) => {
            if *i >= items.len() {
                return panic(format!("index {i} out of bounds (len {})", items.len()));
            }
            Rc::make_mut(items).set(*i, v);
        }
        (_, other) => {
            return panic(format!(
                "cannot index/field-assign into {}",
                other.type_name()
            ));
        }
    }
    Ok(())
}

fn as_int(v: &Value) -> Result<i64, Flow> {
    match v {
        Value::Int(n) => Ok(*n),
        // R19 Slice B: SizedInt values are valid integer values; return the raw stored i64.
        // Builtin operations that receive a SizedInt (e.g. abs_i64, to_str, etc.) get the
        // value as i64 and apply their semantics. This keeps the ~102 builtin Int sites
        // working without modification.
        Value::SizedInt { val, .. } => Ok(*val),
        other => panic(format!("expected i64, got {}", other.type_name())),
    }
}
fn as_float(v: &Value) -> Result<f64, Flow> {
    match v {
        Value::Float(f) => Ok(*f),
        other => panic(format!("expected f64, got {}", other.type_name())),
    }
}
fn as_decimal(v: &Value) -> Result<i128, Flow> {
    match v {
        Value::Decimal(m) => Ok(**m),
        other => panic(format!("expected Decimal, got {}", other.type_name())),
    }
}
fn as_bool(v: &Value) -> Result<bool, Flow> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => panic(format!("expected bool, got {}", other.type_name())),
    }
}
fn as_str(v: &Value) -> Result<&str, Flow> {
    match v {
        Value::Str(s) => Ok(s),
        other => panic(format!("expected str, got {}", other.type_name())),
    }
}
fn as_int_opt(v: &Value) -> Option<i64> {
    match v {
        Value::Int(n) => Some(*n),
        Value::SizedInt { val, .. } => Some(*val),
        _ => None,
    }
}
fn as_float_opt(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

/// Whether deterministic mock-LLM responses are enabled (`AXON_AI_MOCK` set and
/// not "0"/empty). Lets the ASI demos run end-to-end with no API key, no
/// network, and no `asi-runtime` feature — for showcases, CI, and tests.
pub fn ai_mock_enabled() -> bool {
    std::env::var("AXON_AI_MOCK")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

/// Milliseconds since the Unix epoch.
#[cfg(not(target_arch = "wasm32"))]
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// wasm32 (esp. unknown-unknown / the browser) has no `SystemTime` clock —
/// `SystemTime::now()` PANICS there ("time not implemented"), which would trap
/// `Interp::build` (rng seeding) for EVERY program. Return a fixed 0: the RNG
/// then seeds deterministically (fine for a browser playground; a real clock for
/// `now_ms()`/`temporal_*` will arrive via a JS-import host in the R7c binding).
#[cfg(target_arch = "wasm32")]
fn now_ms() -> i64 {
    0
}

// Goal-directed optimization (run_goal*/hill_climb*/introspection) extracted to
// interp/goal.rs (R0 slice 3). Its methods live in a second `impl Interp` block
// there; inherent methods resolve across split impl blocks, so call sites in
// this file are unchanged. (A `mod` must be at module scope, not inside `impl`.)
mod goal;
// Interned names, the per-node name-resolution table and the fn table
// (AX-18), in interp/sym.rs.
mod sym;
use sym::*;
// The static pin analysis of a sealed run (R51 S5, from v0.22 C9 round 6,
// amendment 83): which operator sites a type, or a name, nothing on the
// operator side determined reaches. Built in `Interp::build_with` for a
// sealed run only.
mod pin;
// The runtime taint of a sealed run (R51 S2, from v0.22 C9 round 11,
// amendment 102): the accumulator, the control taints, the builtin classes.
mod taint;
// Its unit tests (R51 S2: the hooks of `eval_t::<true>`, driven directly).
#[cfg(test)]
mod taint_tests;
// The per-frame `Kernel` split's unit tests (R51 S3).
#[cfg(test)]
mod kernel_tests;
// The seal crossing's unit tests (R51 S3: calls between operator and sealed code).
#[cfg(test)]
mod crossing_tests;
// Declared-type conformance at the seal's value boundaries (R51 S3, from
// v0.22 C9 round 4, amendments 53/72/78): casts, closure and channel
// contracts, the operator's dict snapshots.
mod conform;
// The seal sinks inside evaluation (R51 S3): global, name, width, refine,
// type and method edges on a `build_with` sealed `Interp`.
#[cfg(test)]
mod sink_tests;
// The PICK mark and the refusing closure sink (R51 S4: `t_note`, the pick
// edges, `t_check_call_picked`), T8.1's laundering attacks.
#[cfg(test)]
mod pick_tests;
// The receiver dispatch rule's unit tests (R51 S4: `t_check_dispatch`, the
// dispatch-rule and taint-rule test switches, the goal-name refusal).
#[cfg(test)]
mod dispatch_tests;
// Its unit tests (R51 S3: the casts and dict edges, driven directly).
#[cfg(test)]
mod conform_tests;
// Core tree-walking evaluator (eval/eval_block/eval_call/eval_binop/
// match_pattern) extracted to interp/eval.rs (R0 slice 5). Its methods live in a
// second `impl Interp` block there; inherent methods resolve across split impl
// blocks, so this file's call sites (and eval's calls to call_builtin/call_fn)
// are unchanged.
mod eval;
// R50: the bytecode engine for fn bodies (`AXON_ENGINE=vm`): engine selection,
// the compiler and the op loop, in interp/vm/. Compiled code runs against the
// same `Env` the tree-walker uses, and every node it does not lower is a `Tree`
// op that calls `eval`.
mod vm;
pub use vm::{set_engine, Engine, EngineRefusal};
// The builtin dispatch (`call_builtin`, the ~2400-line `match name`) extracted to
// interp/builtins.rs (R0 slice 6). Moved as ONE method into a second `impl Interp`
// block; its function-local `want`/`ok!` travel with it. eval.rs's call to
// call_builtin and call_builtin's calls to goal.rs/eval.rs methods + the parent's
// private Interp fields all resolve across the split impl blocks.
mod builtins;
// Re-exported for CODEGEN, so the caps->effect-row rule has exactly one
// implementation. Native agent_action records carried no effect row until
// the ABI was widened to pass one; re-deriving it on the runtime side would
// have created a second copy of this mapping, free to drift from the
// interpreter's.
// Only CODEGEN consumes this; the interpreter calls the fn directly inside its
// own module. Gated on the same feature, or a --no-default-features build fails
// on an unused import.
#[cfg(feature = "codegen")]
pub(crate) use builtins::cap_to_effect_row;
// `@[forall]` property-testing harness extracted to interp/proptest.rs (R0
// slice 4). Self-contained — its only entry point is the public `run_property_test`,
// re-exported here at the original `interp::` path for main.rs (no unqualified
// internal call sites in this file, so no `use proptest::*` glob needed).
mod proptest;
pub use proptest::{run_property_test, PropertyOutcome};
// Provenance / audit logging extracted to interp/provenance.rs (R0 slice 1).
mod provenance;
// Bring the moved free fns/types back into scope so existing unqualified
// call sites (json_quote, append_*_jsonl, read_best_input, …) are unchanged,
// and re-export the public API at the original `interp::` path for main.rs.
use provenance::*;
// `sha256_hex` is re-exported crate-internally so `replay.rs` can fingerprint a
// journal payload with the SAME hash the provenance/AI-replay caches use, rather
// than adding a second digest implementation that could disagree with them.
pub(crate) use provenance::sha256_hex;
pub use provenance::{
    append_run_start_jsonl, best_recorded_score, find_run_start, provenance_log_path,
    read_ai_calls, read_provenance, set_provenance_source, set_session_cell, AiCallRecord,
    ProvRecord, RunStartRecord,
};

/// A pseudo-random `u64` from a process-global xorshift state (seeded from the
/// clock on first use). Single-threaded interpreter, so no CAS needed.
use std::sync::atomic::{AtomicU64, Ordering as AtomicOrdering};

/// Global xorshift64 RNG state: the OPERATOR kernel's stream (the sealed
/// kernel has its own, `Kernel::rng`). `0` means "uninitialized" — the first
/// draw seeds it (see [`rng_seed`]). Explicitly settable via [`set_rand_seed`]
/// (the `srand` builtin) for reproducible runs. Read and written only by
/// `Kernel::rng_next` / `Kernel::rng_set`, so every draw goes through
/// [`Interp::rng_next`].
static RNG_STATE: AtomicU64 = AtomicU64::new(0);

/// Parse the ambient run-level token cap from `AXON_BUDGET_TOKENS`.
///
/// Unset means no cap. A malformed value FAILS CLOSED to `Some(0)` — no AI at
/// all — rather than falling back to no-cap, because this is a safety control
/// an operator sets from outside the program: a typo (`1O000`) silently
/// disarming the cap is the same inert-surface failure the var had before
/// anything read it, just one level down. The warning names the bad value so
/// the typo is recoverable in one look.
///
/// A negative value clamps to 0, which is meaningful and distinct from unset.
fn parse_token_budget() -> Option<i64> {
    let raw = std::env::var("AXON_BUDGET_TOKENS").ok()?;
    let t = raw.trim();
    match t.parse::<i64>() {
        Ok(n) => Some(n.max(0)),
        Err(_) => {
            eprintln!(
                "warning: AXON_BUDGET_TOKENS={raw:?} is not an integer — failing closed \
                 to a budget of 0 (no AI calls). Set a whole number of tokens, or unset \
                 the variable for no cap."
            );
            Some(0)
        }
    }
}

/// Build the ambient sandbox from `AXON_ALLOWED_EFFECTS`, or an empty registry
/// when the var is unset.
///
/// The var is the ambient counterpart to `sandbox_create`: a comma-separated
/// effect ceiling for the whole run, for a caller who cannot edit the program to
/// wrap it in an explicit sandbox. It was documented as "enforced at runtime"
/// and nothing read it, so `AXON_ALLOWED_EFFECTS=Pure` let an FS write through
/// and exited 0 — a control surface that reads as a safety mechanism while a run
/// configured with it behaves exactly like an unconfigured one.
///
/// The enforcement was never the missing part: `call_builtin`'s F5 hook already
/// refuses any effect outside the active sandbox's set (SandboxViolation, exit
/// 8). It is gated on `active_sandbox >= 0` and nothing ambient ever set that.
/// So this registers a sandbox and makes it active, reusing the same
/// `SandboxEntry` and the same check as `sandbox_create` rather than adding a
/// second enforcement path that could drift from it.
///
/// An EMPTY value is meaningful and is NOT the same as unset:
/// `AXON_ALLOWED_EFFECTS=` means "pure only, deny every effect", while unset
/// means "no ceiling at all". Distinguishing them matters because
/// deny-everything is a case a caller reaches for deliberately.
///
/// The scope is unscoped (`Default`) — this grants effects without a path/host
/// restriction, matching plain `sandbox_create`. Bound to principal handle 0 so
/// audit attribution matches the rest of the run.
fn ambient_sandbox() -> Vec<SandboxEntry> {
    let Ok(raw) = std::env::var("AXON_ALLOWED_EFFECTS") else {
        return Vec::new();
    };
    let allowed: std::collections::HashSet<String> = raw
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    // A misspelled entry is silently a grant of NOTHING. That fails closed,
    // which is the right direction — but it also means an operator who writes
    // `AXON_ALLOWED_EFFECTS=IO,Exce` believes they granted `Exec` and is told
    // otherwise only if the program happens to spawn. A grant that was never
    // valid, recorded as granted: say so at parse time, the way
    // `AXON_BUDGET_TOKENS` already does for a malformed value.
    let mut unknown: Vec<&String> = allowed
        .iter()
        .filter(|e| !crate::builtins::is_grantable_effect(e))
        .collect();
    if !unknown.is_empty() {
        unknown.sort();
        for e in unknown {
            eprintln!(
                "warning: AXON_ALLOWED_EFFECTS names `{e}`, which is not an effect — \
                 it grants nothing. Valid names: {}",
                crate::builtins::GRANTABLE_EFFECTS.join(", ")
            );
        }
    }
    vec![SandboxEntry {
        principal: 0,
        allowed,
        scope: SandboxScope::default(),
    }]
}

/// Initial seed for the RNG. Reproducibility (BUG_HUNT #11 / I-10):
/// uses `AXON_SEED` (parsed as u64) when set for a deterministic run,
/// otherwise time-based entropy. A fixed seed makes every `random_*`,
/// `goal_run_random`, and `goal_run_multistart` result replayable.
fn rng_seed() -> u64 {
    if let Ok(s) = std::env::var("AXON_SEED") {
        if let Ok(n) = s.trim().parse::<u64>() {
            return n | 1; // avoid the 0 "uninitialized" sentinel
        }
    }
    (now_ms() as u64) | 1
}

/// Explicitly set the operator RNG seed (`srand(n)` in an operator frame).
/// `n == 0` is mapped to a non-zero sentinel so it doesn't read as
/// "uninitialized".
fn set_rand_seed(n: i64) {
    let s = (n as u64) | 1;
    RNG_STATE.store(s, AtomicOrdering::Relaxed);
}

/// The operator kernel's next draw (see [`RNG_STATE`]).
fn next_rand_u64() -> u64 {
    let mut x = RNG_STATE.load(AtomicOrdering::Relaxed);
    if x == 0 {
        x = rng_seed();
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    RNG_STATE.store(x, AtomicOrdering::Relaxed);
    x
}

/// Render `n` in `base` (2–36), '-'-prefixed when negative.
fn i64_to_radix(n: i64, base: u32) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let neg = n < 0;
    let mut v = (n as i128).unsigned_abs(); // u128 — handles i64::MIN
    let b = base as u128;
    let mut buf = Vec::new();
    while v > 0 {
        buf.push(DIGITS[(v % b) as usize]);
        v /= b;
    }
    if neg {
        buf.push(b'-');
    }
    buf.reverse();
    String::from_utf8(buf).unwrap()
}

/// A numeric value coerced to `f64` for scoring, if it is numeric.
fn numeric_score(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::SizedInt { val, .. } => Some(*val as f64),
        Value::Float(f) => Some(*f),
        // An `@[adaptive]` fn that returns `Uncertain<T>`/`Temporal<T>` (e.g. an
        // AI scorer whose score carries a confidence) scores on its INNER value —
        // the same soft-typing rule as everywhere else. Without this, the
        // optimizer recorded no score for such a fn and `goal_run` silently fell
        // back to the target (no optimization happened at all).
        _ => value::soft_inner(v).and_then(|inner| numeric_score(&inner)),
    }
}

/// Split a total eval budget into (n_starts, evals_per_start) for multistart —
/// roughly sqrt(N) starts so each gets a meaningful local refinement budget.
fn split_budget(total: i64) -> (i64, i64) {
    let t = total.max(1);
    let starts = ((t as f64).sqrt().floor() as i64).max(2).min(t);
    let per = (t / starts).max(1);
    (starts, per)
}

/// The optimization strategy a `#[goal(strategy: …)]` selects (PRD L889-899).
/// A closed set — an unknown name is E1505 (validated by the checker).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoalStrategy {
    /// Gradient-style local search (the default). Maps to `run_goal`.
    HillClimb,
    /// Uniform random sampling of the `[lo, hi)` box. Maps to `run_goal_random`.
    Random,
    /// Random restarts + local refinement. Maps to `run_goal_multistart`.
    Multistart,
    /// Generational: sample a population, keep the top-K, mutate around them,
    /// repeat. Good for multi-modal objectives. Maps to `run_goal_tournament`.
    Tournament,
    /// Exploit-biased search: multistart that spends most of its budget
    /// refining the best basin found. (A true Gaussian-process surrogate is
    /// out of v1 scope; this is the honest exploit-heavy approximation —
    /// documented as such, never claimed to be a GP.) Maps to a tournament with
    /// a single elite + heavy local refinement.
    Bayesian,
}

impl GoalStrategy {
    fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "hill_climb" | "hillclimb" => Some(GoalStrategy::HillClimb),
            "random" => Some(GoalStrategy::Random),
            "multistart" => Some(GoalStrategy::Multistart),
            "tournament" => Some(GoalStrategy::Tournament),
            "bayesian" => Some(GoalStrategy::Bayesian),
            _ => None,
        }
    }
}

/// The parsed `#[goal(...)]` attribute (R5 sugar).
struct GoalSpec {
    metric: String,
    target: f64,
    max_evals: i64,
    holdout_set: Vec<i64>,
    strategy: GoalStrategy,
    lo: i64,
    hi: i64,
}

/// Build an `Uncertain { value, confidence }` struct value with
/// `source_tag = 0` (user-constructed).
fn make_uncertain(value: Value, confidence: f64) -> Value {
    make_uncertain_tagged(value, confidence, SRC_TAG_USER)
}

/// An `Uncertain` whose value came from a MODEL (`ai_extract_uncertain_*`, on
/// the mock, replay and live paths alike). Stamps 1, as codegen does. These
/// paths used `make_uncertain` and so stamped 0 — a model's answer read as
/// user-constructed under `axon run`, the fail-open direction for a provenance
/// field (UPGRADE_V0_20.md D-014).
fn make_uncertain_ai(value: Value, confidence: f64) -> Value {
    make_uncertain_tagged(value, confidence, SRC_TAG_AI)
}

/// `source_tag` values, as stamped by codegen. These are OBSERVABLE — the
/// checker lists `source_tag` as a field of `Uncertain<T>` and both engines
/// let a program read `u.source_tag` — so the interpreter must stamp the same
/// number codegen does, or a program that branches on provenance takes a
/// different branch under `axon run` than under `axon build`.
pub(crate) const SRC_TAG_USER: i64 = 0;
pub(crate) const SRC_TAG_AI: i64 = 1;
pub(crate) const SRC_TAG_RUNTIME: i64 = 2;

/// Build an `Uncertain { value, confidence, source_tag }` struct value.
///
/// `source_tag` (0=user-constructed, 1=AI-sourced, 2=runtime) is a field of the
/// Uncertain struct — the checker lists it as a valid field and codegen builds
/// the 3-field `{value, confidence, source_tag}` layout. Without it here,
/// `u.source_tag` type-checked but panicked at runtime ("no field source_tag")
/// and the interp's 2-field struct diverged from codegen's 3-field one.
///
/// It was then HARDCODED to 0 for every construction path, which diverged
/// again in the opposite direction: codegen stamps 2 for the `uncertain_dyn_*`
/// constructors, so `uncertain_dyn_f64(1.0, 0.5).source_tag` printed 0 under
/// the interpreter and 2 natively. The interpreter — the reference engine and
/// the default execution path — was reporting a runtime-sourced value as
/// user-constructed, which is the fail-open direction for a provenance field.
fn make_uncertain_tagged(value: Value, confidence: f64, source_tag: i64) -> Value {
    Value::record(
        SYM_UNCERTAIN,
        Fields::from_pairs([
            (SYM_VALUE, value),
            (SYM_CONFIDENCE, Value::Float(confidence)),
            (SYM_SOURCE_TAG, Value::Int(source_tag)),
        ]),
    )
}

/// Build a `Temporal { value, confidence, horizon_ms, decay, created_ms,
/// valid_until_ms }` struct value. `confidence` is the present trust in the value
/// (1.0 at creation), which `temporal_at` decays as time advances (PRD
/// §"Temporal"). `created_ms` is internal (read by temporal_at/is_valid);
/// `valid_until_ms` = created_ms + horizon_ms is the user-facing expiry timestamp
/// the checker exposes as a field — without it, `t.valid_until_ms` type-checked
/// then panicked "no field valid_until_ms" (a checker-only phantom field).
fn make_temporal(
    value: Value,
    confidence: f64,
    horizon_ms: i64,
    decay: f64,
    created_ms: i64,
) -> Value {
    Value::record(
        SYM_TEMPORAL,
        Fields::from_pairs([
            (SYM_VALUE, value),
            (SYM_CONFIDENCE, Value::Float(confidence.clamp(0.0, 1.0))),
            (intern("horizon_ms"), Value::Int(horizon_ms)),
            (intern("decay"), Value::Float(decay)),
            (intern("created_ms"), Value::Int(created_ms)),
            (
                intern("valid_until_ms"),
                Value::Int(created_ms.saturating_add(horizon_ms)),
            ),
        ]),
    )
}

fn is_i64_type(ty: &crate::ast::AxonType) -> bool {
    matches!(ty, crate::ast::AxonType::Named(n) if n == "i64")
}

/// An `@[adaptive]` fn's return type is i64-SCORED when it's `i64` OR a soft
/// wrapper around i64 (`Uncertain<i64>`/`Temporal<i64>`) — the optimizer reads
/// the score from the inner value (numeric_score unwraps it). Without this, an
/// AI scorer `-> Uncertain<i64>` wasn't recognized as i64-returning, so goal_run
/// never entered the hill-climb and silently returned the target (no optimization).
fn is_i64_scored_ret(ty: &crate::ast::AxonType) -> bool {
    use crate::ast::AxonType::*;
    match ty {
        Named(n) => n == "i64",
        Generic { base, args } if (base == "Uncertain" || base == "Temporal") => {
            args.first().map(is_i64_type).unwrap_or(false)
        }
        _ => false,
    }
}

/// Same as `is_i64_scored_ret` for f64.
fn is_f64_scored_ret(ty: &crate::ast::AxonType) -> bool {
    use crate::ast::AxonType::*;
    match ty {
        Named(n) => n == "f64",
        Generic { base, args } if (base == "Uncertain" || base == "Temporal") => {
            args.first().map(is_f64_type).unwrap_or(false)
        }
        _ => false,
    }
}

fn is_f64_type(ty: &crate::ast::AxonType) -> bool {
    matches!(ty, crate::ast::AxonType::Named(n) if n == "f64")
}

/// Apply a comparison `BinOp` to two floats (used by the `@[verify]` gate).
fn cmp_f64(op: &BinOp, a: f64, b: f64) -> bool {
    match op {
        BinOp::Gt => a > b,
        BinOp::GtEq => a >= b,
        BinOp::Lt => a < b,
        BinOp::LtEq => a <= b,
        BinOp::Eq => a == b,
        BinOp::NotEq => a != b,
        _ => false,
    }
}

/// A scalar `Value` as an `f64` for a `@[verify(value OP K)]` comparison, or
/// `None` for a non-scalar (struct/enum/array/…), which `value` can't bind to.
/// `bool` maps to 0.0/1.0 so `@[verify(value == 1)]` works on a flag.
fn scalar_as_f64(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::SizedInt { val, .. } => Some(*val as f64),
        Value::Float(f) => Some(*f),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn eval_unary(op: &UnaryOp, v: Value) -> R {
    match (op, v) {
        (UnaryOp::Neg, Value::Int(n)) => Ok(Value::Int(-n)),
        (UnaryOp::Neg, Value::SizedInt { val, ty }) => Ok(Value::SizedInt {
            val: val.wrapping_neg(),
            ty,
        }),
        (UnaryOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
        (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
        (UnaryOp::BitNot, Value::Int(n)) => Ok(Value::Int(!n)),
        (UnaryOp::BitNot, Value::SizedInt { val, ty }) => Ok(Value::SizedInt { val: !val, ty }),
        // `&expr` is a no-op at runtime for a value interpreter.
        (UnaryOp::Ref, v) => Ok(v),
        (op, v) => panic(format!("cannot apply {op:?} to {}", v.type_name())),
    }
}

/// Pull `(inner_value, confidence)` out of a value that may or may not be an
/// `Uncertain { value, confidence }` struct. A non-Uncertain value carries an
/// implicit confidence of 1.0 — so `uncertain(5, 0.8) + 3` treats `3` as
/// certain. Returns `None` for a value that isn't Uncertain (the caller then
/// uses it as-is with confidence 1.0). Mirrors codegen's `extract` closure
/// (`asi.rs::emit_binop_uncertain`) so the interpreter and native agree.
// Value formatting + value-level ops extracted to interp/value.rs (R0 slice 2).
mod value;

/// R42 Slice 5: the Pike VM regex engine. Its own module because it is a real
/// compiler+VM rather than a builtin body, and because its refusals
/// (backreferences, lookaround, oversized counted repetition) are a security
/// boundary worth reading in one place.
mod regex;
use value::*;

// ── R19 Slice B — coercion helpers ────────────────────────────────────────────

/// The width an `AxonType` annotation names, but only for non-i64 fixed-width
/// integer types. `None` for i64 (no coercion needed — `Int(i64)` is already
/// the correct representation) and for all non-integer types.
fn axon_type_to_width(ty: &crate::ast::AxonType) -> Option<IntWidth> {
    match ty {
        crate::ast::AxonType::Named(n) => match n.as_str() {
            "u8" => Some(IntWidth::U8),
            "u16" => Some(IntWidth::U16),
            "u32" => Some(IntWidth::U32),
            "u64" => Some(IntWidth::U64),
            "i8" => Some(IntWidth::I8),
            "i16" => Some(IntWidth::I16),
            "i32" => Some(IntWidth::I32),
            _ => None, // i64 and non-integer names: no coercion
        },
        _ => None,
    }
}

/// Coerce a runtime value to a `SizedInt` when the target type is a non-i64
/// integer. `Int(n)` → `SizedInt{n, ty}`. Any other value is returned as-is
/// (the type-checker has already validated the types match; this is a
/// representation upgrade only). Outside a sealed run a `SizedInt` with a
/// different width is re-tagged to the new width (preserves the stored
/// bit-pattern; the checker ensures same-width ops only). In a sealed run
/// (`SEALED`) it is left as it is, and the declared-type cast that follows
/// refuses it: re-tagging it would turn one width into another without
/// converting its value, before the cast could see it (v0.22 C9 round 4b,
/// amendment 60; R51 T7, M921).
fn coerce_to_sized<const SEALED: bool>(v: Value, width: IntWidth) -> Value {
    match v {
        Value::Int(n) => Value::SizedInt { val: n, ty: width },
        Value::SizedInt { val, .. } if !SEALED => Value::SizedInt { val, ty: width },
        other => other,
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> i32 {
        let program = crate::parse_source(src).expect("parse failed");
        // Unmapped: these tests use `main`'s return as a value channel — the
        // cheapest way to read a computed result out of a program — and the
        // status rule is about what a PROCESS reports, not what a program
        // computed. The rule has its own tests, plus `exit_code_parity.sh`.
        run_program_unmapped(&program)
    }

    /// AUDIT T35 (finding RT-02). The static checker validates `ai_complete`
    /// against the implicit host `api.anthropic.com`, but the runtime resolved
    /// AXON_AI_BASE_URL / ANTHROPIC_BASE_URL at call time — two independent
    /// values. Reproduced end-to-end: a program whose
    /// `@[contained(net: ["api.anthropic.com"])]` passed `axon check` exit 0 sent
    /// its prompt and the real x-api-key to a listener on 127.0.0.1, via a `.env`
    /// one directory ABOVE it. This is the collection half of the pin.
    #[test]
    fn declared_net_hosts_collects_every_contained_grant() {
        let p = crate::parse_source(
            r#"
@[contained(net: ["api.anthropic.com"])]
fn ask() -> i64 { 0 }
fn main() { }
"#,
        )
        .expect("parse failed");
        let hosts = declared_net_hosts(&p);
        assert_eq!(hosts, vec!["api.anthropic.com".to_string()]);
    }

    /// A program that declares no net grant is NOT pinned — it never made a
    /// claim to violate, and pinning it would break legitimate self-hosted
    /// gateway workflows that rely on ANTHROPIC_BASE_URL.
    #[test]
    fn a_program_with_no_net_grant_is_not_pinned() {
        let p = crate::parse_source(r#"fn main() { println("hi") }"#).expect("parse failed");
        assert!(
            declared_net_hosts(&p).is_empty(),
            "an unannotated program must not be constrained by the pin"
        );
    }

    /// The implicit AI host is added so a program that passed the checker can
    /// still reach the endpoint the checker approved it for.
    #[test]
    fn the_implicit_ai_host_is_always_included_once_a_grant_exists() {
        let p = crate::parse_source(
            r#"
@[contained(net: ["gw.trusted.io"])]
fn fetch() -> i64 { 0 }
fn main() { }
"#,
        )
        .expect("parse failed");
        let hosts = declared_net_hosts(&p);
        assert!(hosts.contains(&"gw.trusted.io".to_string()));
        assert!(
            hosts.contains(&crate::capabilities::ai_implicit_host().to_string()),
            "the host the checker validates against must be reachable: {hosts:?}"
        );
    }

    #[test]
    fn hello_runs_and_exits_zero() {
        assert_eq!(run(r#"fn main() { println("hi") }"#), 0);
    }

    #[test]
    fn main_returns_exit_code() {
        // 49, not 7: 7 is GOAL_BUDGET_EXIT_CODE, and a value falling out of
        // `main` may no longer claim a ledger code. That this test had to change
        // is itself the finding — the assertion was using a reserved number as
        // an ordinary return value.
        assert_eq!(run("fn main() -> i64 { 49 }"), 49);
    }

    #[test]
    fn a_returned_answer_may_not_impersonate_a_guard() {
        // The whole point of the ledger is that a supervisor can branch on it.
        // A program whose ANSWER happens to be 6 must not be readable as a
        // refinement violation, or 11 as a replay divergence — otherwise any
        // program's arithmetic can forge the one signal the supervisor trusts.
        for reserved in [2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 101] {
            let (code, complaint) = returned_exit_status(reserved);
            assert_eq!(code, 1, "a returned {reserved} must not exit {reserved}");
            assert!(
                complaint.is_some_and(|m| m.contains("RESERVED")),
                "and it must say why it did not"
            );
        }
        // The ordinary ground either side of the ledger is untouched.
        for ok in [0, 1, 16, 49, 125, 200, 255] {
            assert_eq!(returned_exit_status(ok), (ok as i32, None));
        }
    }

    #[test]
    fn a_returned_answer_may_not_truncate_silently() {
        // A status is one byte. `main` returning 3240 was OBSERVED as 168: the
        // number the program produced was not the number anyone saw, and nothing
        // said so. Refusing to exit at all is not an option (the process must
        // exit with something), so it exits 1 and explains itself.
        let (code, complaint) = returned_exit_status(3240);
        assert_eq!(code, 1);
        let msg = complaint.expect("silent truncation is the bug — it must speak");
        assert!(
            msg.contains("168"),
            "the message must name what the caller WOULD have seen, or it does \
             not explain the surprise: {msg}"
        );
        // Negative values are the same hazard wearing another hat.
        assert_eq!(returned_exit_status(-1).0, 1);
        assert_eq!(returned_exit_status(256).0, 1);
        // 126..=255 are the SHELL's convention, not this project's ledger, and
        // ordinary answers land there — they pass through.
        assert_eq!(returned_exit_status(200), (200, None));
        assert_eq!(returned_exit_status(126), (126, None));
    }

    #[test]
    fn a_stated_status_is_honoured_including_ledger_codes() {
        // `exit(n)` is a deliberate claim about the process status, so userland
        // keeps the ledger vocabulary — this is how a deploy gate written in
        // Axon says "policy rejection" with the same code the @[verify] gate
        // uses (BUG_HUNT #26/#34). Taking that away would have broken the
        // one-class-per-rejection design while claiming to protect it.
        for n in [0, 1, 3, 6, 11, 49, 101, 200, 255] {
            assert_eq!(
                stated_exit_status(n),
                (n as i32, None),
                "exit({n}) is a statement of intent and must be honoured as written"
            );
        }
        // What is still refused is a value that is not a status at all.
        let (code, complaint) = stated_exit_status(3240);
        assert_eq!(code, 1);
        assert!(complaint.is_some_and(|m| m.contains("168")));
    }

    #[test]
    fn the_two_rules_differ_only_where_intent_differs() {
        // The load-bearing distinction, stated as an assertion rather than left
        // to prose: on the ledger block the two rules must DISAGREE (stated is
        // honoured, returned is not), and everywhere else they must agree.
        for n in 2..=15 {
            assert_ne!(
                stated_exit_status(n).0,
                returned_exit_status(n).0,
                "the ledger block is exactly where stating a status differs from \
                 producing a value; if these agree, one of the two rules is wrong"
            );
        }
        for n in [0, 1, 16, 49, 125, 3240, -1] {
            assert!(
                stated_exit_status(n).0 == returned_exit_status(n).0,
                "outside the reserved ranges the rules must not diverge (n={n})"
            );
        }
    }

    // ── R15 resume runtime (v0) — suspend/resume across a host driver ──────────
    fn parse(src: &str) -> crate::ast::Program {
        crate::parse_source(src).expect("parse failed")
    }

    #[test]
    fn r15_host_await_single_roundtrip() {
        // B1: the request reaches the host, and the host's reply flows back into
        // the program. host("ab") → "abab"; str_len("abab") = 4.
        let prog = parse(r#"fn main() -> i64 { let r = host_await("ab")  str_len(r) }"#);
        let code = super::run_suspendable_unmapped(&prog, |req| Some(format!("{req}{req}")));
        assert_eq!(code, 4);
    }

    #[test]
    fn r15_host_await_effects_fire_once_not_per_resume() {
        // B2 (the load-bearing test): the host is called EXACTLY once per
        // host_await — three awaits ⇒ three host calls. A replay-based suspend
        // would re-run the prefix and call the host MORE than three times; the
        // coroutine/thread substrate suspends in place, so it's exactly three.
        let prog = parse(
            "fn main() -> i64 { let a = host_await(\"1\")  let b = host_await(\"2\")  let c = host_await(\"3\")  str_len(a) + str_len(b) + str_len(c) }",
        );
        let mut calls = 0;
        let code = super::run_suspendable_unmapped(&prog, |_req| {
            calls += 1;
            Some("ok".to_string()) // len 2
        });
        assert_eq!(
            calls, 3,
            "host must be called exactly once per host_await (no replay)"
        );
        assert_eq!(code, 6); // 2 + 2 + 2
    }

    #[test]
    fn r15_host_await_loop_n_times() {
        // B3: a while-loop of host_await — the common interactive shape (an event /
        // prompt loop). The host feeds a different reply each iteration; the
        // program accumulates and the host is called exactly N times (loop count).
        let prog = parse(
            "fn main() -> i64 { let total = 0  let i = 0  while i < 3 { let s = host_await(\"w\")  total = total + str_len(s)  i = i + 1 }  total }",
        );
        let replies = ["ab", "cde", "f"]; // lengths 2, 3, 1
        let mut n = 0;
        let code = super::run_suspendable_unmapped(&prog, |_req| {
            let r = replies[n].to_string();
            n += 1;
            Some(r)
        });
        assert_eq!(n, 3, "host called once per loop iteration");
        assert_eq!(code, 6, "2 + 3 + 1");
    }

    #[test]
    fn r15_no_await_runs_unchanged() {
        // B4: a program that never suspends runs to completion under the driver,
        // identically to a bare run, with zero host calls.
        let prog = parse("fn main() -> i64 { 2 + 3 }");
        let mut calls = 0;
        let code = super::run_suspendable_unmapped(&prog, |_| {
            calls += 1;
            Some(String::new())
        });
        assert_eq!(code, 5);
        assert_eq!(calls, 0, "no host_await ⇒ no suspension");
    }

    #[test]
    fn r15_panic_mid_suspend_does_not_hang() {
        // Robustness: a program that ERRORS after a host_await must return cleanly
        // (the host loop ends when the worker drops its channels), never hang. Here
        // `10 / str_len("")` is a runtime div-by-zero (exit 101) after one await.
        let prog =
            parse("fn main() -> i64 { let g = host_await(\"x\")  let z = str_len(\"\")  10 / z }");
        let code = super::run_suspendable_unmapped(&prog, |_| Some("ok".to_string()));
        assert_eq!(code, 101, "interp panic mid-suspend → exit 101, no hang");
    }

    #[test]
    fn r15_host_await_opt_none_at_eof_terminates_loop() {
        // EOF semantics: `host_await_opt` returns None once the host signals
        // end-of-input (the closure returns None), so a read loop terminates
        // instead of spinning. The host feeds 2 lines then EOF; the program counts
        // the Some replies and stops on None. 2 inputs ⇒ exit 2.
        let prog = parse(
            "fn main() -> i64 { let n = 0  let go = 1  while go == 1 { match host_await_opt(\"?\") { None => { go = 0 } Some(s) => { n = n + 1 } } }  n }",
        );
        let mut fed = 0;
        let code = super::run_suspendable_unmapped(&prog, |_| {
            fed += 1;
            if fed <= 2 {
                Some("x".to_string())
            } else {
                None
            } // 2 lines, then EOF
        });
        assert_eq!(code, 2, "two Some replies then None ⇒ loop stops at 2");
    }

    #[test]
    fn r15_host_await_collapses_eof_to_empty_string() {
        // The simple str form maps EOF (host None) to "" — back-compat for
        // fixed-exchange programs that don't distinguish end-of-input.
        let prog = parse(r#"fn main() -> i64 { let r = host_await("x")  str_len(r) }"#);
        let code = super::run_suspendable_unmapped(&prog, |_| None); // immediate EOF
        assert_eq!(code, 0, "EOF ⇒ host_await returns \"\" ⇒ len 0");
    }

    #[test]
    fn r15_host_await_without_host_errors_cleanly() {
        // A bare `run` (no driver) must error gracefully (exit 101), not hang.
        let prog = parse(r#"fn main() -> i64 { let r = host_await("x")  str_len(r) }"#);
        assert_eq!(super::run_program(&prog), 101);
    }

    // ── R15 Slice 2 — arbitrary-`Value` payloads survive a suspend ─────────────

    #[test]
    fn r15_slice2_dict_payload_round_trips() {
        // The case that did NOT work before Slice 2: a DICT crosses the suspend.
        // The program builds a dict, yields it via host_await_val, the host inspects
        // the request (a dict), and replies with a (different) dict; the program
        // reads a key out of the reply. Proves the !Send `Value::Dict` deep-clones
        // across the worker thread and reconstructs on both directions.
        let prog = parse(
            "fn main() -> i64 { let d = dict_new()  dict_set(d, \"a\", 7)  let r = host_await_val(d)  dict_get_or(r, \"b\", 0) }",
        );
        let mut saw_request_a = 0;
        let code = super::run_suspendable_values_unmapped(&prog, |req| {
            // The request must be a Dict carrying a=7 (the deep-clone preserved it).
            if let SendValue::Dict(entries) = &req {
                for (k, v) in entries {
                    if k == "a" {
                        if let SendValue::Int(n) = v {
                            saw_request_a = *n;
                        }
                    }
                }
            }
            // Reply with a fresh dict {b: 42}.
            Some(SendValue::Dict(vec![("b".to_string(), SendValue::Int(42))]))
        });
        assert_eq!(saw_request_a, 7, "host saw the request dict's a=7");
        assert_eq!(code, 42, "program read b=42 from the reply dict");
    }

    #[test]
    fn r15_slice2_struct_payload_round_trips() {
        // A STRUCT crosses the suspend: the program builds a Point, yields it, the
        // host reads its fields and replies with a Point whose x is the sum; the
        // program reads the reply's x field.
        let prog = parse(
            "type Point = { x: i64, y: i64 }\nfn main() -> i64 { let p = Point { x: 3, y: 4 }  let r = host_await_val(p)  r.x }",
        );
        let mut sum = 0;
        let code = super::run_suspendable_values_unmapped(&prog, |req| {
            if let SendValue::Struct { name, fields } = &req {
                assert_eq!(name, "Point");
                let mut x = 0;
                let mut y = 0;
                for (k, v) in fields {
                    if let SendValue::Int(n) = v {
                        if k == "x" {
                            x = *n;
                        } else if k == "y" {
                            y = *n;
                        }
                    }
                }
                sum = x + y;
            }
            Some(SendValue::Struct {
                name: "Point".to_string(),
                fields: vec![
                    ("x".to_string(), SendValue::Int(sum)),
                    ("y".to_string(), SendValue::Int(0)),
                ],
            })
        });
        assert_eq!(sum, 7, "host summed the struct fields");
        assert_eq!(code, 7, "program read x=7 back from the reply struct");
    }

    #[test]
    fn r15_slice2_enum_and_tuple_payload_round_trip() {
        // Enum + tuple payloads cross too (nested composite). The program yields an
        // Option (an enum-like sum) and the host replies with Some(99).
        let prog = parse(
            "fn main() -> i64 { let r = host_await_val_opt(Some(5))  match r { Some(n) => n  None => -1 } }",
        );
        let code = super::run_suspendable_values_unmapped(&prog, |req| {
            // Request is Some(5).
            assert!(matches!(&req, SendValue::Some(b) if matches!(**b, SendValue::Int(5))));
            Some(SendValue::Int(99))
        });
        assert_eq!(code, 99, "program unwrapped Some(99) from the reply");
    }

    #[test]
    fn r15_slice2_chan_payload_is_refused_not_corrupted() {
        // SOUNDNESS BOUNDARY: a Chan (identity-shared mutable state) cannot cross a
        // suspend without silently losing its sharing, so it is REFUSED (a clean
        // runtime panic, exit 101) rather than deep-cloned into a disconnected copy.
        let prog = parse("fn main() -> i64 { let c = chan<i64>()  let r = host_await_val(c)  0 }");
        // The host is never reached (the refusal happens at the boundary, worker-side).
        let mut host_calls = 0;
        let code = super::run_suspendable_values_unmapped(&prog, |_req| {
            host_calls += 1;
            Some(SendValue::Int(0))
        });
        assert_eq!(
            code, 101,
            "Chan payload → clean refusal (exit 101), not corruption"
        );
        assert_eq!(host_calls, 0, "the refused payload never reached the host");
    }

    #[test]
    fn r15_slice2_str_form_still_works_through_value_substrate() {
        // Regression: the str-typed host_await still works now that the substrate
        // carries SendValue (str crosses as SendValue::Str). The B1 case, unchanged.
        let prog = parse(r#"fn main() -> i64 { let r = host_await("ab")  str_len(r) }"#);
        let code = super::run_suspendable_unmapped(&prog, |req| Some(format!("{req}{req}")));
        assert_eq!(
            code, 4,
            "str host_await round-trips through the SendValue channel"
        );
    }

    #[test]
    fn r15_slice2_send_value_round_trip_is_lossless() {
        // Unit-level: from_value ∘ into_value preserves a deeply nested composite
        // (dict in struct in array). Chan-free, so it must succeed and reconstruct
        // an equal-shaped Value.
        let mut inner = std::collections::BTreeMap::new();
        inner.insert("k".to_string(), Value::Int(9));
        let dict = Value::Dict(Rc::new(RefCell::new(inner.into())));
        let s = Value::record(
            intern("S"),
            Fields::from_pairs([(intern("d"), dict), (intern("n"), Value::Int(1))]),
        );
        let v = Value::Array(Rc::new(
            vec![s, Value::Str(Rc::new("hi".to_string()))].into(),
        ));
        let sv = SendValue::from_value(&v).expect("Chan-free ⇒ sendable");
        let back = sv.into_value();
        // Spot-check the reconstructed shape.
        if let Value::Array(xs) = &back {
            assert_eq!(xs.len(), 2);
            if let Some(Value::Struct(sv)) = xs.first() {
                assert_eq!(&*sym_name(sv.name), "S");
                assert!(matches!(sv.fields.named("n"), Some(Value::Int(1))));
                if let Some(Value::Dict(d)) = sv.fields.named("d") {
                    assert!(matches!(d.borrow().get("k"), Some(Value::Int(9))));
                } else {
                    panic!("dict field lost");
                }
            } else {
                panic!("struct lost");
            }
        } else {
            panic!("array lost");
        }
    }

    #[test]
    fn fmt_g_matches_c_printf_six_g() {
        // R1f slice 2b: the interpreter's fmt_g must match C's `%.6g` (the format
        // the native codegen path emits via snprintf) so I-2 holds — the
        // differential fuzzer (fuzz_parity.sh) caught the original divergence.
        // Pins both the mantissa trailing-zero trim AND the signed two-digit
        // exponent in the scientific-notation branch, plus -0.0 normalization.
        assert_eq!(fmt_g(1_000_000.0), "1e+06");
        assert_eq!(fmt_g(1_234_567.0), "1.23457e+06");
        assert_eq!(fmt_g(9_999_999.0), "1e+07");
        assert_eq!(fmt_g(0.000_000_1), "1e-07");
        assert_eq!(fmt_g(-1_234_567.0), "-1.23457e+06");
        assert_eq!(fmt_g(1.5e15), "1.5e+15");
        assert_eq!(fmt_g(-2.5e-12), "-2.5e-12");
        // Non-scientific range is unchanged.
        assert_eq!(fmt_g(123_456.0), "123456");
        assert_eq!(fmt_g(0.0001), "0.0001");
        assert_eq!(fmt_g(2.71875), "2.71875");
        // Zero (and -0.0) normalize to "0".
        assert_eq!(fmt_g(0.0), "0");
        assert_eq!(fmt_g(-0.0), "0");
    }

    #[test]
    fn arithmetic_and_while() {
        let src = r#"
            fn main() -> i64 {
                let acc = 0
                let i = 1
                while i <= 10 { acc = acc + i  i = i + 1 }
                acc
            }
        "#;
        assert_eq!(run(src), 55);
    }

    #[test]
    fn recursion() {
        let src = r#"
            fn fib(n: i64) -> i64 {
                if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
            }
            fn main() -> i64 { fib(10) }
        "#;
        assert_eq!(run(src), 55);
    }

    // BUG_HUNT #22: parse_int error messages are specific, not generic.
    #[test]
    fn parse_int_radix_prefix_errs_with_hint() {
        // Returns 1 (Err arm) for hex; the message is checked via the CLI test.
        let src = r#"
            fn main() -> i64 {
                match parse_int("0xFF") { Ok(_) => 0  Err(_) => 1 }
            }
        "#;
        assert_eq!(run(src), 1, "hex literal must Err (base-10 only)");
    }

    #[test]
    fn parse_int_still_trims_and_parses_decimal() {
        let src = r#"
            fn main() -> i64 {
                match parse_int("  -123  ") { Ok(n) => n  Err(_) => 0 }
            }
        "#;
        assert_eq!(run(src), -123, "leading/trailing space still trims");
    }

    // BUG_HUNT #25: the generated deploy-gate symbol must not leak to users.
    #[test]
    fn verify_label_hides_generated_deploy_gate_name() {
        assert_eq!(verify_fn_label("assert_deployable"), "the deploy gate");
        assert!(
            !verify_fn_label("assert_deployable").contains("assert_deployable"),
            "the internal symbol must not appear in the founder-facing label"
        );
    }

    #[test]
    fn verify_label_keeps_author_named_functions() {
        // An author's own @[verify] fn keeps its name — it's meaningful to them.
        assert_eq!(verify_fn_label("safety_gate"), "`safety_gate`");
        assert_eq!(verify_fn_label("gate"), "`gate`");
    }

    // BUG_HUNT #27: random_i64 rejects inverted bounds and stays in range.
    #[test]
    fn random_i64_valid_bounds_stay_in_range() {
        // Deterministic via srand; sample several draws, all in [lo, hi).
        let src = r#"
            fn main() -> i64 {
                srand(42)
                let bad = 0
                let i = 0
                while i < 50 {
                    let r = random_i64(10, 20)
                    if r < 10 { bad = bad + 1 }
                    if r >= 20 { bad = bad + 1 }
                    i = i + 1
                }
                bad
            }
        "#;
        assert_eq!(run(src), 0, "all draws must fall in [10, 20)");
    }

    #[test]
    fn random_i64_inverted_bounds_is_panic() {
        let src = "fn main() -> i64 { random_i64(20, 10) }";
        // Graceful panic → exit 101 (not a silent return of `lo`).
        assert_eq!(run(src), 101);
    }

    // BUG_HUNT #28: AXON_MAX_DEPTH resolution and stack-coupling are pure and
    // unit-testable without mutating process-global env.
    #[test]
    fn max_depth_defaults_when_unset_or_malformed() {
        assert_eq!(max_depth_from_env(None), RECURSION_LIMIT);
        assert_eq!(max_depth_from_env(Some("")), RECURSION_LIMIT);
        assert_eq!(max_depth_from_env(Some("banana")), RECURSION_LIMIT);
        assert_eq!(
            max_depth_from_env(Some("0")),
            RECURSION_LIMIT,
            "zero is not a useful limit"
        );
        assert_eq!(
            max_depth_from_env(Some("-5")),
            RECURSION_LIMIT,
            "negatives don't parse as usize"
        );
    }

    #[test]
    fn max_depth_honors_valid_value_and_trims() {
        assert_eq!(max_depth_from_env(Some("9000")), 9000);
        assert_eq!(max_depth_from_env(Some("  12345  ")), 12345);
    }

    #[test]
    fn max_depth_clamps_to_ceiling() {
        assert_eq!(max_depth_from_env(Some("999999999999")), MAX_DEPTH_CEILING);
    }

    #[test]
    fn stack_grows_with_depth_but_never_below_floor() {
        // Shallow limits keep the MIN_STACK_BYTES floor. The crossover is
        // MIN_STACK_BYTES / STACK_BYTES_PER_FRAME frames; below it, floored.
        let crossover = MIN_STACK_BYTES / STACK_BYTES_PER_FRAME; // 4096
        assert_eq!(stack_size_for_depth(1), MIN_STACK_BYTES);
        assert_eq!(stack_size_for_depth(crossover), MIN_STACK_BYTES);
        // Past the crossover, the stack scales with depth so the guard stays
        // ahead of a real overflow.
        let deep = 100_000;
        assert!(deep > crossover);
        assert_eq!(stack_size_for_depth(deep), deep * STACK_BYTES_PER_FRAME);
        assert!(stack_size_for_depth(deep) > MIN_STACK_BYTES);
        // The default limit (6000 > 4096) already scales above the floor.
        assert!(stack_size_for_depth(RECURSION_LIMIT) > MIN_STACK_BYTES);
        // The ceiling can't overflow the multiply (saturating) — it produces a
        // finite budget at or above the floor, capped at 5864c423's largest
        // reserve, which never shrinks below 5864c423's reserve at any depth.
        assert!(stack_size_for_depth(MAX_DEPTH_CEILING) >= MIN_STACK_BYTES);
        assert_eq!(stack_size_for_depth(MAX_DEPTH_CEILING), MAX_STACK_BYTES);
        for d in [
            1,
            4096,
            RECURSION_LIMIT,
            100_000,
            800_000,
            900_000,
            MAX_DEPTH_CEILING,
        ] {
            let base = (d * 256 * 1024).max(1024 * 1024 * 1024);
            assert!(stack_size_for_depth(d) >= base, "depth {d}");
        }
    }

    // BUG_HUNT #20: dict_to_str must return Result<str,str>, not panic the
    // host, when a key/value can't be represented in the line format. The
    // caller can then recover.
    #[test]
    fn dict_to_str_bad_key_returns_err_not_panic() {
        let src = r#"
            fn main() -> i64 {
                let d = dict_new()
                dict_set(d, "a=b", "v")
                match dict_to_str(d) { Ok(_) => 0  Err(_) => 7 }
            }
        "#;
        // 7 = the Err arm ran. A host panic would exit 101 instead.
        assert_eq!(run(src), 7);
    }

    #[test]
    fn dict_to_str_newline_value_returns_err_not_panic() {
        let src = r#"
            fn main() -> i64 {
                let d = dict_new()
                dict_set(d, "k", "line1\nline2")
                match dict_to_str(d) { Ok(_) => 0  Err(_) => 7 }
            }
        "#;
        assert_eq!(run(src), 7);
    }

    #[test]
    fn dict_to_str_clean_dict_round_trips() {
        let src = r#"
            fn main() -> i64 {
                let d = dict_new()
                dict_set(d, "k", "v")
                match dict_to_str(d) {
                    Ok(s) => {
                        let back = dict_from_str(s)
                        match dict_get(back, "k") { Some(_) => 1  None => -1 }
                    }
                    Err(_) => -2
                }
            }
        "#;
        assert_eq!(run(src), 1);
    }

    #[test]
    fn result_match_and_question() {
        let src = r#"
            fn half(n: i64) -> Result<i64, str> {
                if n % 2 == 0 { Ok(n / 2) } else { Err("odd") }
            }
            fn doit(n: i64) -> Result<i64, str> {
                let h = half(n)?
                Ok(h + 1)
            }
            fn main() -> i64 {
                match doit(10) { Ok(v) => v  Err(_) => -1 }
            }
        "#;
        assert_eq!(run(src), 6);
    }

    #[test]
    fn enum_match() {
        let src = r#"
            enum Shape { Circle { r: i64 }, Square { s: i64 } }
            fn area(x: Shape) -> i64 {
                match x { Shape::Circle { r } => r * r  Shape::Square { s } => s * s }
            }
            fn main() -> i64 { area(Shape::Square { s: 4 }) }
        "#;
        assert_eq!(run(src), 16);
    }

    #[test]
    fn closures_capture() {
        let src = r#"
            fn make_adder(n: i64) -> (i64) -> i64 { (x: i64) => x + n }
            fn main() -> i64 {
                let add10 = make_adder(10)
                add10(5)
            }
        "#;
        assert_eq!(run(src), 15);
    }

    #[test]
    fn for_loop_break_continue() {
        let src = r#"
            fn main() -> i64 {
                let total = 0
                for i in 0..10 {
                    if i == 5 { break }
                    total = total + i
                }
                total
            }
        "#;
        assert_eq!(run(src), 10); // 0+1+2+3+4
    }

    #[test]
    fn struct_field_access() {
        let src = r#"
            type Point = { x: i64, y: i64 }
            fn main() -> i64 {
                let p = Point { x: 3, y: 4 }
                p.x + p.y
            }
        "#;
        assert_eq!(run(src), 7);
    }

    // ── ASI (M2) ──────────────────────────────────────────────────────────

    #[test]
    fn uncertain_construction_and_field_access() {
        let src = r#"
            fn main() -> i64 {
                let u = uncertain_new(42, 0.75)
                u.value
            }
        "#;
        assert_eq!(run(src), 42);
    }

    #[test]
    fn numeric_conversions() {
        let src = r#"
            fn main() -> i64 { f64_to_i64(i64_to_f64(9) + 0.9) }
        "#;
        assert_eq!(run(src), 9); // 9.9 truncates toward zero
    }

    #[test]
    fn goal_run_hill_climbs_adaptive_fn() {
        // score(x) = 100 - (x-7)^2, peak 100 at x=7. goal_run should climb to it.
        let src = r#"
            @[adaptive(metric: s, target: 100)]
            fn score(x: i64) -> i64 {
                let d = x - 7
                100 - d * d
            }
            fn main() -> i64 {
                let best = goal_run("score", 100.0, 64)
                f64_to_i64(best)
            }
        "#;
        assert_eq!(run(src), 100);
    }

    #[test]
    fn goal_run_retrospective_picks_closest() {
        // measure returns f64 → not hill-climb-eligible → retrospective path
        // over logged records [20, 40, 60]; closest to 50 is 40 (earliest of tie).
        let src = r#"
            @[adaptive(metric: m, target: 50)]
            fn measure(x: i64) -> f64 { i64_to_f64(x) }
            fn main() -> i64 {
                let _ = measure(20)
                let _ = measure(40)
                let _ = measure(60)
                f64_to_i64(goal_run("measure", 50.0, 100))
            }
        "#;
        assert_eq!(run(src), 40);
    }

    #[test]
    fn goal_run_errors_on_unknown_name() {
        // BUG_HUNT #19 / I-9: a name that is neither a defined fn nor in the
        // provenance store is a typo. It must error (exit 101), NOT silently
        // return the target as if the goal were achieved. (This test
        // previously asserted the bug — return-target — as correct.)
        let src = r#"
            fn main() -> i64 { f64_to_i64(goal_run("never_called", 70.0, 20)) }
        "#;
        assert_eq!(run(src), 101);
    }

    #[test]
    fn goal_run_retrospective_lookup_still_works() {
        // The legitimate fallthrough: an adaptive fn that HAS run can be
        // re-queried by name (max_evals=0) and returns its best observed.
        let src = r#"
            @[adaptive]
            fn s(x: i64) -> i64 { x }
            fn main() -> i64 {
                let _ = goal_run("s", 100.0, 20)
                f64_to_i64(goal_run("s", 100.0, 0))
            }
        "#;
        assert_eq!(run(src), 100);
    }

    #[test]
    fn verify_gate_passes_when_confident() {
        let src = r#"
            @[verify(confidence >= 0.8)]
            fn gate(c: f64) -> Uncertain<i64> { uncertain_dyn_i64(1, c) }
            fn main() -> i64 {
                let u = gate(0.9)
                u.value
            }
        "#;
        assert_eq!(run(src), 1);
    }

    #[test]
    fn verify_gate_panics_when_underconfident() {
        let src = r#"
            @[verify(confidence >= 0.8)]
            fn gate(c: f64) -> Uncertain<i64> { uncertain_dyn_i64(1, c) }
            fn main() -> i64 {
                let u = gate(0.5)
                u.value
            }
        "#;
        // verify gate fires → distinct policy exit code 3, NOT a crash 101
        // (BUG_HUNT #26).
        assert_eq!(run(src), VERIFY_FAILED_EXIT_CODE);
    }

    #[test]
    fn extended_math_builtins() {
        // clamp_i64 / sign_i64 / pow_i64 / min_f64 / max_f64 / clamp_f64
        let src = r#"
            fn main() -> i64 {
                let i = clamp_i64(150, 0, 100) + clamp_i64(-5, 0, 100) + sign_i64(-7) + pow_i64(2, 10)
                // 100 + 0 + (-1) + 1024 = 1123
                let f = f64_to_i64(min_f64(3.5, 2.5) + max_f64(1.0, 4.0) + clamp_f64(9.9, 0.0, 5.0))
                // 2.5 + 4.0 + 5.0 = 11.5 -> 11
                i + f
            }
        "#;
        assert_eq!(run(src), 1134);
    }

    #[test]
    fn more_builtins_coverage() {
        // sqrt_f64 / round_f64 / str_pad_start / str_pad_end / i64_to_str_radix / uncertain_new_f64
        let src = r#"
            fn main() -> i64 {
                let a = f64_to_i64(sqrt_f64(144.0))                  // 12
                let b = f64_to_i64(round_f64(2.6))                   // 3
                let p = str_len(str_pad_start("7", 4, "0"))          // "0007" -> 4
                let q = str_len(str_pad_end("7", 2, "x"))            // "7x" -> 2
                let r = if str_eq(i64_to_str_radix(255, 16), "ff") { 1 } else { 0 } // 1
                let u = f64_to_i64(uncertain_new_f64(5.0, 0.9).value) // 5
                a + b + p + q + r + u
            }
        "#;
        assert_eq!(run(src), 27);
    }

    #[test]
    fn ai_mock_mode_returns_ok() {
        // With AXON_AI_MOCK set, ai_complete returns Ok(non-empty) with no key /
        // network / asi-runtime feature. (No other test reads this env var.)
        std::env::set_var("AXON_AI_MOCK", "1");
        let n = run(r#"
            fn main() -> i64 {
                match ai_complete("anything") {
                    Ok(s) => str_len(s)
                    Err(_) => -1
                }
            }
        "#);
        std::env::remove_var("AXON_AI_MOCK");
        assert!(
            n > 0,
            "mock ai_complete should return Ok(non-empty), got {n}"
        );
    }

    #[test]
    fn goal_demos_pure_outcomes() {
        // Regression lock for the key-free goal demos: prose → .ax → run,
        // pinning each gate path. (No LLM / no env — the mock-requiring goals
        // hello/flagship are exercised separately.)
        let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/goals/");
        for (file, expected) in [
            ("optimize-goal.md", 0), // deploys
            ("compose-goal.md", 0),  // deploys (prelude-composed score)
            // enforced confidence gate blocks → distinct verify-failed code 3,
            // not a crash 101 (BUG_HUNT #26).
            ("verified-goal.md", VERIFY_FAILED_EXIT_CODE),
            // redteam gate blocks → the SAME policy-rejection code as the verify
            // gate (3), not 1 — every deploy-gate rejection is one exit class
            // (BUG_HUNT #34 surface follow-on to #26).
            ("redteam-goal.md", VERIFY_FAILED_EXIT_CODE),
        ] {
            let md = std::fs::read_to_string(format!("{base}{file}"))
                .unwrap_or_else(|e| panic!("read {file}: {e}"));
            let goal = axon_surface::parser::GoalFile::parse(&md)
                .unwrap_or_else(|e| panic!("parse {file}: {e}"));
            let ax =
                axon_surface::compile::emit(&goal).unwrap_or_else(|e| panic!("emit {file}: {e}"));
            let program =
                crate::parse_source(&ax).unwrap_or_else(|e| panic!("parse .ax for {file}: {e}"));
            let code = run_program(&program);
            assert_eq!(
                code, expected,
                "{file}: expected exit {expected}, got {code}"
            );
        }
    }

    #[test]
    fn sandbox_run_enforces_effect_ceiling_at_runtime_f5() {
        // F5 gate: sandbox_run must deny a builtin whose effect row is outside
        // the sandbox's declared ceiling (exit 8, SANDBOX_VIOLATION_EXIT_CODE),
        // and must allow builtins whose effects are within the ceiling.

        // Case 1: sandbox allows only "IO" — random_i64 ("Random") is denied.
        let src_denied = r#"
            fn noisy(_x: i64) -> i64 { random_i64(1, 100) }
            fn main() -> i64 {
                let p = principal_root("test", false, false, false, 100)
                let sb = sandbox_create(p, "IO")
                sandbox_run(sb, "noisy", 0)
            }
        "#;
        assert_eq!(
            run(src_denied),
            SANDBOX_VIOLATION_EXIT_CODE,
            "sandbox_run should exit {} when the fn calls random_i64 but Random is not allowed",
            SANDBOX_VIOLATION_EXIT_CODE
        );

        // Case 2: sandbox allows "Random" — random_i64 is permitted; result is non-error.
        let src_allowed = r#"
            fn make_rand(_x: i64) -> i64 {
                let r = random_i64(1, 10)
                r
            }
            fn main() -> i64 {
                let p = principal_root("test2", false, false, false, 100)
                let sb = sandbox_create(p, "Random")
                let v = sandbox_run(sb, "make_rand", 0)
                if v >= 1 && v <= 10 { 0 } else { 1 }
            }
        "#;
        std::env::set_var("AXON_SEED", "42");
        let result = run(src_allowed);
        std::env::remove_var("AXON_SEED");
        assert_eq!(
            result, 0,
            "sandbox_run should succeed and return a value in [1,10] when Random is allowed"
        );
    }

    #[test]
    fn sandbox_create_and_run_pure_fn_f5() {
        // A pure fn (no effects) always runs inside any sandbox, including an empty ceiling.
        // sandbox_run always passes 1 i64 arg, so the fn must accept it.
        let src = r#"
            fn double(x: i64) -> i64 { x * 2 }
            fn main() -> i64 {
                let p = principal_root("pure", false, false, false, 100)
                let sb = sandbox_create(p, "")
                sandbox_run(sb, "double", 21)
            }
        "#;
        assert_eq!(
            run(src),
            42,
            "pure fn should run inside an empty-ceiling sandbox"
        );
    }

    #[test]
    fn all_example_ax_files_parse() {
        // Broad regression guard: every .ax under examples/ must parse (covers
        // the basic examples, the asi "public face", stdlib, and modular libs).
        fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        collect(&p, out);
                    } else if p.extension().map(|x| x == "ax").unwrap_or(false) {
                        out.push(p);
                    }
                }
            }
        }
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples"));
        let mut files = Vec::new();
        collect(root, &mut files);
        assert!(
            files.len() >= 20,
            "expected many example .ax files, found {}",
            files.len()
        );
        for f in &files {
            let src =
                std::fs::read_to_string(f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
            if let Err(e) = crate::parse_source(&src) {
                panic!("{} failed to parse: {e}", f.display());
            }
        }
    }

    // ── K4: run_suspendable_hypercall (Unix-socket / hypercall substrate) ───────

    /// Without a live socket, a program that never calls `host_await` should
    /// complete normally — the socket is only opened on the first suspension, so
    /// a simple `main` that returns 0 is indistinguishable from a bare `run_program`.
    #[test]
    #[cfg(unix)]
    fn run_suspendable_hypercall_no_host_await_exits_zero() {
        let src = r#"fn main() -> i64 { println("ok") 0 }"#;
        let prog = crate::parse_source(src).expect("parse failed");
        // AXON_HOST_SOCKET may or may not be set; the default socket
        // (/tmp/axon-host.sock) very likely doesn't exist in the test
        // environment. Neither matters: the program never calls host_await,
        // so unix_socket_roundtrip is never invoked.
        let code = super::run_suspendable_hypercall(&prog);
        assert_eq!(code, 0);
    }

    // ── R51 S0b: veto's non-seal interpreter fixes, on every engine ────────

    /// `name`'s outcome as [`run_test_fn_outcome`] reports it, on one engine
    /// (`eager`: the VM compiles every body on its first entry).
    fn outcome_on(
        prog: &Program,
        name: &str,
        engine: Engine,
        eager: bool,
    ) -> Result<TestEnd, String> {
        on_deep_stack(|| {
            let mut ix = Interp::build(prog)?;
            ix.engine = engine;
            ix.vm_eager = eager;
            if let Err(f) = ix.init_globals() {
                return Err(flow_to_msg(f));
            }
            let Some(f) = ix.fns.get(name).copied() else {
                return Err(format!("no function `{name}`"));
            };
            ix.test_outcome(f)
        })
    }

    /// `name`'s outcome on the tree, which the eager VM and the default VM
    /// must reproduce exactly (R51 §3: each fix holds on both engines).
    fn outcome(prog: &Program, name: &str) -> Result<TestEnd, String> {
        let tree = outcome_on(prog, name, Engine::Tree, false);
        for eager in [true, false] {
            assert_eq!(
                outcome_on(prog, name, Engine::Vm, eager),
                tree,
                "{name}: the VM (eager: {eager}) disagrees with the tree"
            );
        }
        tree
    }

    /// v0.22 G01 final re-audit: a `break` escaping a function called by a
    /// test used to count as a clean pass, so candidate code could end an
    /// acceptance test before its assertion ran. It is a failure now; a test
    /// that completes still passes.
    #[test]
    fn an_escaped_break_or_continue_does_not_pass_a_test() {
        let prog = crate::parse_source(
            "fn stop(n: i64) -> i64 {\n    if n > 0 { break }\n    n\n}\n\
             fn skip(n: i64) -> i64 {\n    if n > 0 { continue }\n    n\n}\n\
             @[test]\nfn t_break() { assert_eq(stop(1), 99) }\n\
             @[test]\nfn t_continue() { assert_eq(skip(1), 99) }\n\
             @[test]\nfn t_ok() { assert_eq(stop(0), 0) }\n",
        )
        .expect("parses");
        assert!(outcome(&prog, "t_break").is_err());
        assert!(outcome(&prog, "t_continue").is_err());
        assert!(outcome(&prog, "t_ok").is_ok());
        // Candidate 2's blocker: the escape lands in a loop the TEST owns. It
        // must not end that loop and skip the assertions (while, for, and
        // through a closure).
        let prog = crate::parse_source(
            "fn stop(n: i64) -> i64 {\n    if n > 0 { break }\n    n\n}\n\
             fn skip(n: i64) -> i64 {\n    if n > 0 { continue }\n    n\n}\n\
             @[test]\nfn t_while() {\n    let i = 1\n    while i < 4 {\n        assert_eq(stop(i), 99)\n        i = i + 1\n    }\n}\n\
             @[test]\nfn t_for() {\n    for i in 1..4 {\n        assert_eq(skip(i), 99)\n    }\n}\n\
             @[test]\nfn t_closure() {\n    let g = |n: i64| { if n > 0 { break }  n }\n    for i in 1..4 {\n        assert_eq(g(i), 99)\n    }\n}\n\
             @[test]\nfn t_own_loop_ok() {\n    let i = 0\n    while true {\n        i = i + 1\n        if i > 2 { break }\n    }\n    assert_eq(i, 3)\n}\n",
        )
        .expect("parses");
        for t in ["t_while", "t_for", "t_closure"] {
            assert!(outcome(&prog, t).is_err(), "{t} passed");
        }
        assert_eq!(
            outcome(&prog, "t_own_loop_ok"),
            Ok(TestEnd::Completed),
            "a test's own break still works"
        );
        // The property-test runner had the same hole.
        let prog = crate::parse_source(
            "fn stop(n: i64) -> i64 {\n    break\n    n\n}\n\
             @[test]\n@[forall]\nfn p(n: i64) { assert_eq(stop(n), 99) }\n",
        )
        .expect("parses");
        assert!(
            matches!(
                proptest::run_property_test(&prog, "p", 5),
                proptest::PropertyOutcome::Failed { .. }
            ),
            "an escaped break passed a property case"
        );
    }

    /// M58's own property: a `break`/`continue` raised in a named FUNCTION
    /// BODY does not escape into the caller's loop. Each caller loops, so on
    /// the VM the later calls run through the fast call paths.
    #[test]
    fn an_escaped_break_or_continue_from_a_function_body_does_not_pass_a_test() {
        let prog = crate::parse_source(
            "fn stop(n: i64) -> i64 {\n    if n > 0 { break }\n    n\n}\n\
             fn skip(n: i64) -> i64 {\n    if n > 0 { continue }\n    n\n}\n\
             @[test]\nfn t_break() { assert_eq(stop(1), 99) }\n\
             @[test]\nfn t_continue() { assert_eq(skip(1), 99) }\n\
             @[test]\nfn t_while() {\n    let i = 1\n    while i < 4 {\n        assert_eq(stop(i), 99)\n        i = i + 1\n    }\n}\n\
             @[test]\nfn t_for() {\n    for i in 1..4 {\n        assert_eq(skip(i), 99)\n    }\n}\n\
             @[test]\nfn t_ok() { assert_eq(stop(0), 0) }\n",
        )
        .expect("parses");
        for t in ["t_break", "t_continue", "t_while", "t_for"] {
            assert!(
                outcome(&prog, t).is_err(),
                "ATTACK: `{t}` passed: a break/continue escaped a function body"
            );
        }
        assert!(outcome(&prog, "t_ok").is_ok(), "control: t_ok");
    }

    /// Candidate 4's final-review blocker (executed): a `break` in a candidate
    /// function's parameter/return refinement, `@[verify]` predicate, or a
    /// candidate type's struct refinement escaped the call and ended the
    /// operator test's loop. Loop control is contained at every frame edge now.
    #[test]
    fn loop_control_does_not_escape_through_a_predicate() {
        let cases = [
            (
                "return refinement",
                "fn solve(n: i64) -> (i64 where if n > 0 { break } else { true }) { 0 }\n",
            ),
            (
                "param refinement",
                "fn solve(n: i64 where if n > 0 { break } else { true }) -> i64 { 0 }\n",
            ),
            (
                "verify",
                "@[verify(if value == 0 { break } else { true })]\nfn solve(n: i64) -> i64 { 0 }\n",
            ),
        ];
        let mut passed = Vec::new();
        for (why, def) in cases {
            let src = format!(
                "{def}@[test]\nfn t() {{\n    let i = 1\n    while i < 4 {{\n        assert_eq(solve(i), 42)\n        i = i + 1\n    }}\n}}\n"
            );
            let prog = crate::parse_source(&src).expect("parses");
            if outcome(&prog, "t").is_ok() {
                passed.push(why);
            }
        }
        let prog = crate::parse_source(
            "type Arg = { n: i64 } where if _.n > 0 { break } else { true }\n\
             @[test]\nfn t() {\n    for i in 1..4 {\n        let a = Arg { n: i }\n        assert_eq(a.n, 42)\n    }\n}\n",
        )
        .expect("parses");
        if outcome(&prog, "t").is_ok() {
            passed.push("struct refinement");
        }
        assert!(
            passed.is_empty(),
            "these escapes passed the test: {passed:?}"
        );
    }

    /// PCI 11/12/13: the affirmative completion point. `run_test_fn_outcome`
    /// reports the early ends as `EndedEarly`, never `Completed`; `axon test`
    /// fails the ones that ended on a failure value (R51 §3, `t_err`/`t_q`).
    #[test]
    fn a_test_completes_only_when_its_body_returns_normally() {
        let prog = crate::parse_source(
            "fn bail(n: i64) -> i64 {\n    if n > 0 { exit(0) }\n    n\n}\n\
             fn early(n: i64) -> i64 {\n    if n > 0 { return 5 }\n    n\n}\n\
             @[test]\nfn t_done() { assert_eq(early(0), 0) }\n\
             @[test]\nfn t_exit() { assert_eq(bail(1), 99) }\n\
             @[test]\nfn t_err() -> Result<i64, str> { Err(\"no\") }\n\
             @[test]\nfn t_q() -> Result<i64, str> {\n    let n = parse_int(\"x\")?\n    assert_eq(n, 99)\n    Ok(n)\n}\n\
             @[test]\nfn t_ok() -> Result<i64, str> { Ok(1) }\n\
             @[test]\nfn t_return() { assert_eq(early(1), 99) }\n\
             @[test]\nfn t_closure_return() {\n    let g = |n: i64| { if n > 0 { return 5 }  n }\n    assert_eq(g(1), 99)\n}\n",
        )
        .expect("parses");
        assert_eq!(outcome(&prog, "t_done"), Ok(TestEnd::Completed));
        assert_eq!(outcome(&prog, "t_ok"), Ok(TestEnd::Completed));
        for t in ["t_exit", "t_err", "t_q"] {
            let end = outcome(&prog, t);
            assert!(matches!(end, Ok(TestEnd::EndedEarly(_))), "{t}: {end:?}");
        }
        for t in ["t_err", "t_q"] {
            let end = outcome(&prog, t).expect("ended");
            assert!(end.failure_exit().is_some(), "{t}: {end:?}");
        }
        let exit = outcome(&prog, "t_exit").expect("ended");
        assert!(exit.failure_exit().is_none(), "t_exit: {exit:?}");
        // PCI 8: a callee's `return` ends the callee, never the test — the
        // assertion after it still runs and fails.
        for t in ["t_return", "t_closure_return"] {
            let end = outcome(&prog, t);
            assert!(end.is_err(), "{t}: {end:?}");
        }
        // A property case that `exit(0)`s did not complete either.
        let prog = crate::parse_source(
            "fn bail(n: i64) -> i64 {\n    exit(0)\n    n\n}\n\
             @[test]\n@[forall]\nfn p(n: i64) { assert_eq(bail(n), 99) }\n",
        )
        .expect("parses");
        assert!(
            matches!(
                proptest::run_property_test(&prog, "p", 5),
                proptest::PropertyOutcome::Failed { .. }
            ),
            "an exit(0) property case passed"
        );
    }

    /// S7-UnsealedParity-8/9: an early `return None` fails like a `?` on
    /// `None` (they leave the same trace), and the reason says so rather than
    /// naming only `Err`; a tail `None` completes. `run_test_fn` gives the
    /// verdict `axon test` gives: `Err` exactly for a failure exit, so
    /// `axon improve`'s test-flip check reads the same verdict.
    #[test]
    fn an_early_none_fails_with_its_own_reason_and_run_test_fn_agrees() {
        let prog = crate::parse_source(
            "fn bail(n: i64) -> i64 {\n    if n > 0 { exit(0) }\n    n\n}\n\
             @[test]\nfn t_return_none() -> Option<i64> {\n    let k = 0\n    if k == 0 { return None }\n    Some(1)\n}\n\
             @[test]\nfn t_tail_none() -> Option<i64> { None }\n\
             @[test]\nfn t_err() -> Result<i64, str> { Err(\"no\") }\n\
             @[test]\nfn t_exit() { assert_eq(bail(1), 99) }\n\
             @[test]\nfn t_done() { assert_eq(1, 1) }\n",
        )
        .expect("parses");
        let end = outcome(&prog, "t_return_none").expect("ended");
        let why = end
            .failure_exit()
            .expect("an early `return None` is a failure exit");
        assert!(why.contains("`return None`"), "{why}");
        assert_eq!(outcome(&prog, "t_tail_none"), Ok(TestEnd::Completed));
        for t in ["t_return_none", "t_err"] {
            assert_eq!(
                run_test_fn(&prog, t),
                Err(TEST_ENDED_ON_FAILURE.to_string()),
                "{t}"
            );
        }
        for t in ["t_tail_none", "t_exit", "t_done"] {
            assert_eq!(run_test_fn(&prog, t), Ok(()), "{t}");
        }
    }

    /// C9 round 3, PSV-3: a test is COMPLETED only when its body evaluated to
    /// its end. A `?` on a type-confused `None` returned `None` from the
    /// test, which is not an `Err`, so the returned VALUE read as a
    /// completion. `t_find` is the route where the completion rule is the
    /// ONLY guard: an `Option`-returning test whose `?` meets an honest
    /// `None` returns a well-typed `None`. (Veto ran the candidate sealed;
    /// the rule is the interpreter's, so here suite and candidate are one
    /// unsealed program.)
    #[test]
    fn a_test_ended_by_question_mark_is_never_completed() {
        let suite = "@[test]\nfn t_solve() -> Result<i64, str> {\n    let f = solver()\n    let v = f(21)?\n    assert_eq(v, 42)\n    Ok(v)\n}\n\
                     @[test]\nfn t_find() -> Option<i64> {\n    let v = find(21)?\n    assert_eq(v, 42)\n    Some(v)\n}\n";
        let honest = "fn solver() -> fn(i64) -> Result<i64, str> { |x: i64| Ok(x * 2) }\nfn find(x: i64) -> Option<i64> { Some(x * 2) }\n";
        let err = "fn solver() -> fn(i64) -> Result<i64, str> { |x: i64| Err(\"no\") }\nfn find(x: i64) -> Option<i64> { Some(x) }\n";
        let confused = "fn solver() -> fn(i64) -> Result<i64, str> {\n    |x: i64| {\n        let d = dict_new()\n        let n: Option<i64> = None\n        dict_set(d, \"k\", n)\n        match dict_get(d, \"k\") { Some(v) => v  None => Err(\"u\") }\n    }\n}\nfn find(x: i64) -> Option<i64> { None }\n";
        let run = |cand: &str, t: &str| {
            let prog = crate::parse_source(&format!("{cand}{suite}")).expect("parses");
            outcome(&prog, t)
        };
        for t in ["t_solve", "t_find"] {
            assert_eq!(run(honest, t), Ok(TestEnd::Completed), "control: {t}");
        }
        assert!(
            matches!(run(err, "t_solve"), Ok(TestEnd::EndedEarly(_))),
            "control: an honest Err ends the test early"
        );
        assert!(run(err, "t_find").is_err(), "control: wrong fails");
        for t in ["t_solve", "t_find"] {
            let out = run(confused, t);
            assert!(
                out != Ok(TestEnd::Completed),
                "ATTACK: a test that ended early at `?` was counted complete ({t}): {out:?}"
            );
        }
    }

    /// C9 round 3, PSV-3: a `fn` declared `-> Result` never hands its caller a
    /// `None` (nor an `Option` fn an `Ok`/`Err`). An untyped `dict_get` gives
    /// a free type variable, so a stored `None` type-checks as the `Result`.
    /// Refused at the return boundary, the test FAILS on the confusion
    /// instead of ending early.
    #[test]
    fn a_declared_result_fn_never_returns_an_option() {
        let suite = "@[test]\nfn t_solve() -> Result<i64, str> {\n    let v = solve(21)?\n    assert_eq(v, 42)\n    Ok(v)\n}\n";
        let confused = "fn solve(x: i64) -> Result<i64, str> {\n    let d = dict_new()\n    let n: Option<i64> = None\n    dict_set(d, \"k\", n)\n    match dict_get(d, \"k\") {\n        Some(v) => v\n        None => Err(\"unreachable\")\n    }\n}\n";
        let honest = "fn solve(x: i64) -> Result<i64, str> { Ok(x * 2) }\n";
        let run = |cand: &str, suite: &str, t: &str| {
            let prog = crate::parse_source(&format!("{cand}{suite}")).expect("parses");
            outcome(&prog, t)
        };
        assert_eq!(
            run(honest, suite, "t_solve"),
            Ok(TestEnd::Completed),
            "control"
        );
        let out = run(confused, suite, "t_solve");
        assert!(
            matches!(&out, Err(m) if m.contains("runtime type confusion")),
            "ATTACK: a fn declared -> Result returned a None across its boundary: {out:?}"
        );
        let opt = "@[test]\nfn t_find() -> Option<i64> {\n    let v = find(21)?\n    Some(v)\n}\n";
        let confused_opt = "fn find(x: i64) -> Option<i64> {\n    let d = dict_new()\n    let e: Result<i64, str> = Ok(42)\n    dict_set(d, \"k\", e)\n    match dict_get(d, \"k\") { Some(v) => v  None => None }\n}\n";
        let out = run(confused_opt, opt, "t_find");
        assert!(
            matches!(&out, Err(m) if m.contains("runtime type confusion")),
            "an Option fn returned an Ok across its boundary: {out:?}"
        );
    }

    // ── R51 S5: the seal state, frame provenance and v0.22's pin analysis ────

    /// A program whose `cand` part is loaded from the SEALED directory
    /// `/<tag>-sealed` and whose `suite` part is the operator's; returns it
    /// with that directory.
    fn sealed_program(tag: &str, suite: &str, cand: &str) -> (Program, std::path::PathBuf) {
        use crate::span::intern_source;
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

    /// v0.22's sealed-run harness: run `test` of `suite` with `cand` loaded
    /// from a SEALED directory. v0.22 published that directory process-wide
    /// (`set_sealed_module_dirs` under a test lock); on main every build that
    /// sees a non-empty set is a sealed run, so a published set would seal
    /// every concurrent test's build. The set is handed to this run's build only
    /// ([`Interp::build_in`]). The other layers' tests run with the
    /// receiver dispatch rule off ([`sealed_outcome_rule`]).
    fn sealed_outcome(tag: &str, suite: &str, cand: &str, test: &str) -> Result<TestEnd, String> {
        sealed_outcome_rule(tag, suite, cand, test, false)
    }

    /// `rule`: whether the dispatch rule (amendment 83) is on (v0.22
    /// interp.rs:5765). The switches are per thread and set on the run's own
    /// thread ([`on_deep_stack`] spawns it), which starts with both off.
    fn sealed_outcome_rule(
        tag: &str,
        suite: &str,
        cand: &str,
        test: &str,
        rule: bool,
    ) -> Result<TestEnd, String> {
        let (prog, sdir) = sealed_program(tag, suite, cand);
        // `PSV1T_TAINT_ONLY=1` (a sweep of the whole suite, amendment 102)
        // runs every rule-on test with the STATIC pin analysis off and only the
        // runtime taint rules on: a verdict that changes is a hole in one of the two.
        let taint_only = rule && std::env::var_os("PSV1T_TAINT_ONLY").is_some();
        on_deep_stack(|| {
            DISPATCH_RULE_OFF.with(|c| c.set(!rule || taint_only));
            TAINT_FORCE_ON.with(|c| c.set(taint_only));
            run_test_fn_inner(&prog, test, &[sdir])
        })
    }

    /// S7-Seal-2: at every depth limit, a sealed caller's call of an operator
    /// fn is refused by the call edge in the uniform words, never as
    /// "recursion limit exceeded … in `expected`" (v0.22 ran `seal_call`
    /// before the depth check, veto interp.rs:3581). Built without the static
    /// check, which would refuse the name first. Limits below the call refuse
    /// on the candidate's own recursion (`double`).
    #[test]
    fn a_sealed_caller_at_the_depth_limit_never_reads_an_operator_fn_by_name() {
        let suite = "fn expected(n: i64) -> i64 { n * 2 }\n\
                     @[test]\nfn t() { assert_eq(double(20), 40) }\n";
        let cand = "fn double(n: i64) -> i64 { if n > 0 { double(n - 1) } else { expected(n) } }\n";
        let (prog, sdir) = sealed_program("s7depth", suite, cand);
        let (mut by_depth, mut by_seal) = (0, 0);
        for limit in 1..40 {
            let out = on_deep_stack(|| {
                let mut interp = Interp::build_in(&prog, std::slice::from_ref(&sdir))?;
                interp.max_depth = limit;
                interp.init_globals().map_err(flow_to_msg)?;
                let f = interp.fns.get("t").copied().expect("the suite defines `t`");
                interp.test_outcome(f)
            });
            let m = out.expect_err("the candidate never reaches its answer");
            assert!(!m.contains("in `expected`"), "limit {limit}: {m}");
            if m.contains("recursion limit exceeded") {
                by_depth += 1;
            } else {
                assert_eq!(m, Interp::sealed_no_fn_msg("expected"), "limit {limit}");
                by_seal += 1;
            }
        }
        assert!(by_depth > 0 && by_seal > 0, "{by_depth} / {by_seal}");
    }

    /// v0.22 (C9 round 4c): the operator's `Judge`, whose `u8` impl is the
    /// lenient one, and the candidate's laundering helpers.
    pub(super) const JUDGE8: &str = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nimpl Judge for u8 {\n    fn ok(self: u8) -> bool { true }\n}\n";
    pub(super) const LAUNDER8: &str = "fn narrow(n: i64) -> u8 { n as u8 }\nfn stash(v: u8) -> Dict {\n    let d = dict_new()\n    dict_set(d, \"k\", v)\n    d\n}\n";

    fn u8_or(some: &str, none: &str) -> String {
        format!("match dict_get(stash(narrow(4)), \"k\") {{ Some(v) => {some}  None => {none} }}")
    }

    /// `match <laundered u8> { Some(v) => { a[0] = v } None => {} }` as the
    /// body of a candidate `fill(a: &mut [i64])`.
    fn mut_fill(decl: &str, act: &str) -> String {
        format!(
            "fn fill(a: {decl}) {{\n    {}\n}}\n",
            u8_or(&format!("{{ {act} }}"), "{}")
        )
    }

    /// v0.22's dispatch-rule harness (C9 round 6, amendment 83): `t` of
    /// `suite` under the operator's `Judge`, against `cand` with the
    /// laundering helpers, with the rule ON.
    fn judged_on(tag: &str, suite: &str, cand: &str) -> Result<TestEnd, String> {
        sealed_outcome_rule(
            tag,
            &format!("{JUDGE8}{suite}"),
            &format!("{LAUNDER8}{cand}"),
            "t",
            true,
        )
    }

    fn dispatch_refused(why: &str, suite: &str, cand: &str) {
        let out = judged_on("r6-disp", suite, cand);
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("whose type nothing on the operator side determined")),
            "ATTACK: operator code dispatched on a value of the candidate's chosen type ({why}): {out:?}"
        );
    }

    /// C9 round 7 (amendment 88): the analysis trusts only what the OPERATOR chose.
    fn r7_refused(why: &str, suite: &str, cand: &str) {
        let out = judged_on("r7", suite, cand);
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("whose type nothing on the operator side determined")
                    || m.contains("whose width nothing on the operator side determined")
                    || m.contains("runtime type confusion")),
            "ATTACK: the operator's analysis trusted a type the candidate chose ({why}): {out:?}"
        );
    }

    /// The operator's sandbox suite: `body` runs in a sandbox scope.
    fn sandbox_suite(body: &str) -> String {
        format!("@[test]\nfn t() {{\n    let p = principal_root(\"r\", true, true, true, 100)\n    let sb = sandbox_create(p, \"IO\")\n{body}\n}}\n")
    }

    fn name_refused(what: &str, suite: &str, cand: &str) {
        let out = judged_on("r10-name", suite, cand);
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("a function name nothing on the operator side determined")),
            "ATTACK: the candidate's string chose the operator fn ({what}): {out:?}"
        );
    }

    /// R51 S6.1: the harness's sealed run executes (S5 to S6 it was refused
    /// at build): an honest candidate completes and a wrong one fails, so the
    /// sealed tests below are not vacuous.
    #[test]
    fn the_sealed_harness_runs_the_suite_against_the_candidate() {
        let suite = "@[test]\nfn t() {\n    assert(solve() == 41)\n}\n";
        assert_eq!(
            sealed_outcome("r51-s6-honest", suite, "fn solve() -> i64 { 41 }\n", "t"),
            Ok(TestEnd::Completed),
            "honest candidate"
        );
        assert!(
            sealed_outcome("r51-s6-wrong", suite, "fn solve() -> i64 { 40 }\n", "t").is_err(),
            "wrong candidate"
        );
    }

    /// R51 S5: a sealed build records which definitions the sealed module
    /// defines (by `FnDef` address, global `Sym`, refinement and type name),
    /// which method names the OPERATOR defines, and computes the pins; an
    /// unsealed build computes none of it. Built through `build_with`, the
    /// seal state's input, so the engine choice is not part of the test.
    #[test]
    fn a_sealed_build_records_the_candidates_definitions_and_an_unsealed_one_none() {
        let suite = "type OT = { q: i64 }\ntrait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nlet OG = 5\nfn op() -> i64 { OG }\n@[test]\nfn t() {\n    let v: i64 = 9\n    assert(v.ok())\n}\n";
        let cand = "type CT = { k: i64 }\ntype CPos = i64 where _ > 0\nlet CG = 4\nfn solve() -> i64 { CG }\nimpl Judge for CT {\n    fn ok(self: CT) -> bool { true }\n}\n";
        let (prog, sdir) = sealed_program("r51-s5-seal", suite, cand);
        let addr = |name: &str| {
            prog.items
                .iter()
                .find_map(|it| match it {
                    Item::FnDef(f) if f.name == name => Some(f as *const FnDef as usize),
                    _ => None,
                })
                .expect("fn")
        };
        let impl_ok = |sealed_impl: bool| {
            prog.items
                .iter()
                .find_map(|it| match it {
                    Item::ImplBlock(b)
                        if crate::resolver::span_in_sealed(b.span, std::slice::from_ref(&sdir))
                            == sealed_impl =>
                    {
                        Some(&b.methods[0] as *const FnDef as usize)
                    }
                    _ => None,
                })
                .expect("impl")
        };
        let sealed = Interp::build_with(&prog, std::slice::from_ref(&sdir), Engine::Tree);
        let s = &sealed.seal;
        assert!(s.active);
        let mut fns: Vec<usize> = s.fns.iter().copied().collect();
        fns.sort_unstable();
        let mut want = vec![addr("solve"), impl_ok(true)];
        want.sort_unstable();
        assert_eq!(
            fns, want,
            "the sealed fns are the candidate's fn and impl method"
        );
        assert_eq!(
            s.globals.iter().copied().collect::<Vec<_>>(),
            vec![intern("CG")]
        );
        assert_eq!(
            s.refines.iter().cloned().collect::<Vec<_>>(),
            vec!["CPos".to_string()]
        );
        assert_eq!(
            s.types.iter().cloned().collect::<Vec<_>>(),
            vec!["CT".to_string()]
        );
        assert_eq!(
            s.operator_methods.iter().cloned().collect::<Vec<_>>(),
            vec!["ok".to_string()],
            "the operator's trait and impl define `ok`; the candidate's impl adds nothing"
        );
        let op = sealed.fns["op"];
        let sol = sealed.fns["solve"];
        assert!(sealed.fn_is_sealed(sol) && !sealed.fn_is_sealed(op));
        assert!(
            sealed.pins.is_operator_type("OT") && !sealed.pins.is_operator_type("CT"),
            "the operator's struct is an operator type, the candidate's never is"
        );
        let t = sealed.fns["t"];
        let site = |f: &FnDef| {
            let mut out = None;
            crate::ast::walk_expr(&f.body, &mut |e: &Expr| {
                if let Expr::MethodCall {
                    receiver, method, ..
                } = e
                {
                    out = Some((receiver.as_ref().clone(), method.clone()));
                }
            });
            out.expect("a method call")
        };
        let (r, m) = site(t);
        let owner = t as *const FnDef as usize;
        assert!(
            sealed.pins.determined(owner, &r, &m),
            "the operator's pinned `v.ok()` is determined in a sealed build"
        );
        let unsealed = Interp::build_with(&prog, &[], Engine::Tree);
        let u = &unsealed.seal;
        assert!(!u.active);
        assert!(
            u.fns.is_empty()
                && u.globals.is_empty()
                && u.refines.is_empty()
                && u.types.is_empty()
                && u.operator_methods.is_empty()
        );
        assert!(!unsealed.fn_is_sealed(unsealed.fns["solve"]));
        assert!(
            !unsealed.pins.determined(owner, &r, &m),
            "an unsealed build computes no pins"
        );
    }

    /// R51 S5: `with_frame` sets the frame's provenance for its body only,
    /// counts the frames of each provenance while they run, and restores
    /// both on the way out, nested either way.
    #[test]
    fn with_frame_sets_provenance_for_its_body_and_restores_it() {
        let prog = crate::parse_source("fn main() {}\n").expect("parses");
        let ix = Interp::build_with(&prog, &[], Engine::Tree);
        let state = |ix: &Interp| {
            (
                ix.frame_sealed.get(),
                ix.sealed_frames.get(),
                ix.operator_frames.get(),
            )
        };
        assert_eq!(state(&ix), (false, 0, 0));
        let seen = ix.with_frame(true, || {
            let outer = state(&ix);
            let inner = ix.with_frame(false, || {
                let op = state(&ix);
                let back = ix.with_frame(true, || state(&ix));
                (op, back, state(&ix))
            });
            (outer, inner, state(&ix))
        });
        assert_eq!(
            seen,
            (
                (true, 1, 0),
                ((false, 1, 1), (true, 2, 1), (false, 1, 1)),
                (true, 1, 0)
            )
        );
        assert_eq!(state(&ix), (false, 0, 0));
        assert_eq!(ix.with_frame(true, || 7), 7, "the body's value is returned");
    }

    /// The analysis itself, with the cast out of the picture: a variable passed
    /// as `&mut x` is NOT determined; the same program without the call is.
    #[test]
    fn a_mut_operand_is_never_determined_by_the_pin_analysis() {
        let verdict = |src: &str| {
            let prog = crate::parse_source(src).expect("parses");
            let pins = pin::Pins::build(&prog, &|_| false);
            let mut out = None;
            for item in &prog.items {
                if let Item::FnDef(f) = item {
                    if f.name != "t" {
                        continue;
                    }
                    crate::ast::walk_expr(&f.body, &mut |e: &Expr| {
                        if let Expr::MethodCall {
                            receiver, method, ..
                        } = e
                        {
                            if method == "ok" {
                                out = Some(pins.determined(
                                    f as *const FnDef as usize,
                                    receiver,
                                    method,
                                ));
                            }
                        }
                    });
                }
            }
            out.expect("a call site")
        };
        let base = "trait Judge {\n    fn ok(self) -> bool\n}\nimpl Judge for i64 {\n    fn ok(self: i64) -> bool { self == 9 }\n}\nfn fill(a: &mut [i64]) { a[0] = 9 }\n";
        let with = |call: &str| {
            format!("{base}fn t() {{\n    let mut a = [1, 2, 3]\n    {call}\n    let r = a[0].ok()\n}}\n")
        };
        assert!(
            verdict(&with("let z = 0")),
            "control: a literal array is determined"
        );
        assert!(
            !verdict(&with("fill(&mut a)")),
            "ATTACK: a variable lent as `&mut` was still determined"
        );
        // A union annotation does not pin the one runtime type.
        let un = format!("{base}fn t() {{\n    let a: i64 | u8 = 9\n    let r = a.ok()\n}}\n");
        assert!(
            !verdict(&un),
            "ATTACK: a union annotation pinned the receiver"
        );
    }

    /// The dispatch rule sees a `&mut` operand as OPEN: what the callee left in
    /// it was not chosen by the operator. The honest suite re-pins.
    #[test]
    fn a_mut_operand_is_open_in_the_dispatch_analysis() {
        let unpinned = "@[test]\nfn t() {\n    let mut a = [1, 2, 3]\n    fill(&mut a)\n    assert(a[0].ok())\n}\n";
        let repinned = "@[test]\nfn t() {\n    let mut a = [1, 2, 3]\n    fill(&mut a)\n    let b: [i64] = a\n    assert(b[0].ok())\n}\n";
        let good = "fn fill(a: &mut [i64]) { a[0] = 9 }\n";
        let wrong = "fn fill(a: &mut [i64]) { a[0] = 4 }\n";
        // The cost, stated: even an honest write is refused until re-pinned.
        dispatch_refused("an un-re-pinned read after `&mut`", unpinned, good);
        assert_eq!(
            judged_on("r8-mutopen", repinned, good),
            Ok(TestEnd::Completed),
            "control: GOOD, re-pinned"
        );
        assert!(
            judged_on("r8-mutopen", repinned, wrong).is_err(),
            "control: WRONG fails"
        );
        let out = judged_on("r8-mutopen", repinned, &mut_fill("&mut [i64]", "a[0] = v"));
        assert!(
            out != Ok(TestEnd::Completed),
            "ATTACK: re-pinned suite accepted the u8: {out:?}"
        );
        // The arithmetic arm, unpinned.
        let width = "@[test]\nfn t() {\n    let mut a = [1, 2, 3]\n    fill(&mut a)\n    assert((a[0] << 1) == 254)\n}\n";
        let out = judged_on(
            "r8-mutopen",
            width,
            &mut_fill("&mut [i64]", "a[0] = v").replace("narrow(4)", "narrow(255)"),
        );
        assert!(
            out != Ok(TestEnd::Completed),
            "ATTACK: width arm accepted a `&mut` value: {out:?}"
        );
    }

    /// The structural half of A: pin verdicts are keyed (owner, site text), so
    /// two fns holding IDENTICAL text must never share one. The analysis keeps
    /// them apart by owner; the interpreter must hand the right owner to every
    /// frame that runs operator code (`every_frame_that_runs_stored_operator_code_sets_its_owner`).
    #[test]
    fn identical_site_text_in_two_fns_gets_two_verdicts() {
        use crate::ast::walk_expr;
        let src = "trait J { fn ok(self) -> bool }\nimpl J for i64 { fn ok(self: i64) -> bool { self == 9 } }\nimpl J for u8 { fn ok(self: u8) -> bool { true } }\nfn a() -> bool { let v: i64 = 9\n v.ok() }\nfn b() -> bool { let v = dict_get(dict_new(), \"k\")\n v.ok() }\n";
        let prog = crate::parse_source(src).expect("parses");
        let pins = pin::Pins::build(&prog, &|_| false);
        let mut verdicts = Vec::new();
        for it in &prog.items {
            if let Item::FnDef(f) = it {
                let mut site = None;
                walk_expr(&f.body, &mut |e: &Expr| {
                    if let Expr::MethodCall {
                        receiver, method, ..
                    } = e
                    {
                        site = Some((receiver.as_ref().clone(), method.clone()));
                    }
                });
                let (r, m) = site.expect("a method call");
                verdicts.push((
                    f.name.clone(),
                    pins.determined(f as *const FnDef as usize, &r, &m),
                ));
            }
        }
        assert_eq!(
            verdicts,
            vec![("a".to_string(), true), ("b".to_string(), false)]
        );
    }

    /// Amendment 96, honest-program cost (fail-closed over-refusal, stated): the
    /// VALUE of a `with handler` expression is undetermined to the dispatch rule
    /// (a handler arm may answer with any value), so an operator dispatch on it
    /// is refused until it is pinned. Dispatch on determined values INSIDE a
    /// handler body or arm is unaffected.
    #[test]
    fn the_value_of_a_with_handler_expression_is_undetermined_until_pinned() {
        let h = "with handler { on IO(p) => resume(Ok(\"x\")) } {\n        9\n    }";
        let run = |suite: String| judged_on("r9-wh", &suite, "fn mk() -> i64 { 9 }\n");
        let pinned = format!("@[test]\nfn t() {{\n    let v: i64 = {h}\n    assert(v.ok())\n}}\n");
        assert_eq!(
            run(pinned),
            Ok(TestEnd::Completed),
            "pinned: the cost is only the annotation"
        );
        let bare = format!("@[test]\nfn t() {{\n    let v = {h}\n    assert(v.ok())\n}}\n");
        let out = run(bare);
        assert!(
            matches!(&out, Err(m) if m.contains("nothing on the operator side determined")),
            "the value of a handler expression is undetermined: {out:?}"
        );
        let inside = "@[test]\nfn t() {\n    with handler { on IO(p) => resume(Ok(\"x\")) } {\n        let x: i64 = 9\n        assert(x.ok())\n    }\n}\n".to_string();
        assert_eq!(
            run(inside),
            Ok(TestEnd::Completed),
            "dispatch inside a handler body on a pinned value"
        );
    }

    /// B2: a LOCAL binding named like an operator fn is not that fn.
    #[test]
    fn a_local_binding_named_like_an_operator_fn_is_not_judged_by_it() {
        let suite = "fn f() -> i64 { 5 }\n@[test]\nfn t() {\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"f\") {\n        Some(f) => assert(f().ok())\n        None => assert(false)\n    }\n}\n";
        r7_refused(
            "a closure stored under a new key, called through a local named f",
            suite,
            "fn solve(d: Dict) { dict_set(d, \"f\", || 4 as u8) }\n",
        );
    }

    /// The second blocker, at the call: a call through a name bound in the
    /// local environment goes through that value, and a non-closure is not
    /// callable. It used to fall through to a builtin or fn of the same NAME,
    /// so a confused value in the operator's local `square` ran the
    /// operator's own reference `fn square`. This route crosses no declared
    /// fn type (a `Dict`'s values are untyped), so the cast cannot stand in.
    #[test]
    fn a_call_through_a_local_never_resolves_the_name_elsewhere() {
        let suite = "fn square(n: i64) -> i64 { n * n }\n\
                     @[test]\nfn t() {\n    match dict_get(table(), \"sq\") {\n        Some(square) => assert(square(3) == 9 && square(5) == 25)\n        None => assert(false)\n    }\n}\n\
                     @[test]\nfn t_shadow() {\n    let square = |n: i64| n + 100\n    assert(square(1) == 101)\n}\n\
                     @[test]\nfn t_plain() {\n    let sq = 4\n    assert(square(sq) == 16)\n}\n";
        let table = |v: &str| {
            format!("fn table() -> Dict {{\n    let d = dict_new()\n    dict_set(d, \"sq\", {v})\n    d\n}}\n")
        };
        let run = |v: &str, test: &str| sealed_outcome("f4call", suite, &table(v), test);
        assert_eq!(
            run("|n: i64| n * n", "t"),
            Ok(TestEnd::Completed),
            "control: the right closure"
        );
        assert!(
            run("|n: i64| n + 1", "t").is_err(),
            "control: the wrong closure fails"
        );
        assert_eq!(
            run("0", "t_shadow"),
            Ok(TestEnd::Completed),
            "control: a local closure shadowing a fn is what the name calls"
        );
        assert_eq!(
            run("0", "t_plain"),
            Ok(TestEnd::Completed),
            "control: with no local of that name, the fn is called"
        );
        let out = run("0", "t");
        assert!(
            out != Ok(TestEnd::Completed)
                && matches!(&out, Err(m) if m.contains("is not callable")),
            "ATTACK: a call through a local ran the operator's fn of the same name: {out:?}"
        );
    }

    /// BLOCKER B. A `str` the candidate returned selects which OPERATOR fn a
    /// name-resolving builtin runs. The name argument is a SINK: operator code
    /// must give it a name-pure expression.
    #[test]
    fn a_function_name_the_candidate_chose_never_selects_an_operator_fn() {
        let sbx = |name_expr: &str, pre: &str| {
            format!(
            "fn reference(x: i64) -> i64 {{ x * 2 }}\n{pre}{}",
            sandbox_suite(&format!("    let got = sandbox_run(sb, {name_expr}, 21)\n    assert(got == reference(21))"))
        )
        };
        let with_local =
            |bind: &str| sbx("nm", "").replace("    let got", &format!("    {bind}\n    let got"));
        let run = |suite: &str, cand: &str| judged_on("r10-name", suite, cand);
        let attack = "fn entry() -> str { \"reference\" }\nfn double(x: i64) -> i64 { 0 }\n";
        let honest = "fn entry() -> str { \"double\" }\nfn double(x: i64) -> i64 { x * 2 }\n";
        // Controls: a literal, an operator-built name, an operator constant and a
        // local bound to one all run the candidate's fn, and a wrong one fails keyed.
        for (what, suite) in [
            ("literal", sbx("\"double\"", "")),
            ("operator fn result", sbx("which()", "fn which() -> str { \"dou\" + \"ble\" }\n")),
            ("operator global", sbx("NAME", "let NAME = \"double\"\n")),
            ("local of a literal", with_local("let nm = \"double\"")),
            ("a branch between two literals", with_local("let nm = if reference(1) == 2 { \"double\" } else { \"reference\" }")),
            ("a loop over a literal range", with_local("let names = [\"nope\", \"double\"]\n    let nm = \"x\"\n    for k in 1..2 { nm = names[k] }")),
            ("an interpolation of literals", with_local("let part = \"ble\"\n    let nm = \"dou{part}\"")),
        ] {
            assert_eq!(run(&suite, honest), Ok(TestEnd::Completed), "control: honest via {what}");
            assert!(
                run(&suite, "fn entry() -> str { \"double\" }\nfn double(x: i64) -> i64 { 0 }\n").is_err(),
                "control: wrong fails via {what}"
            );
        }
        name_refused(
            "sandbox_run, a candidate fn's result",
            &sbx("entry()", ""),
            attack,
        );
        name_refused(
            "a local bound to it",
            &with_local("let nm = entry()"),
            attack,
        );
        name_refused(
            "a local annotated str",
            &with_local("let nm: str = entry()"),
            attack,
        );
        name_refused(
            "a parameter of an operator fn",
            &sbx("pick(entry())", "fn pick(s: str) -> str { s }\n"),
            attack,
        );
        name_refused(
            "an operator fn that returns the candidate's",
            &sbx("pick()", "fn pick() -> str { entry() }\n"),
            attack,
        );
        name_refused(
            "an operator fn that returns it by a return statement",
            &sbx("pick()", "fn pick() -> str {\n    if reference(1) == 2 { return entry() }\n    \"double\"\n}\n"),
            attack,
        );
        name_refused(
            "built from the candidate's",
            &sbx("\"re\" + entry()", ""),
            "fn entry() -> str { \"ference\" }\n",
        );
        name_refused(
            "built from a candidate int through to_str",
            &sbx("\"dou\" + to_str(idx())", ""),
            "fn idx() -> i64 { 1 }\nfn double(x: i64) -> i64 { 0 }\n",
        );
        name_refused(
            "a dict read",
            &with_local("let d = dict_new()\n    dict_set(d, \"k\", entry())\n    let nm = match dict_get(d, \"k\") { Some(s) => s  None => \"\" }"),
            attack,
        );
        name_refused(
            "an index chosen by the candidate",
            &sbx("[\"double\", \"reference\"][idx()]", ""),
            "fn idx() -> i64 { 1 }\nfn double(x: i64) -> i64 { 0 }\n",
        );
        name_refused(
            "a loop variable",
            &sbx("names[i]", "").replace("    let got", "    let names = [\"double\", \"reference\"]\n    let i = 0\n    for k in 0..idx() { i = k }\n    let got"),
            "fn idx() -> i64 { 1 }\nfn double(x: i64) -> i64 { 0 }\n",
        );
    }

    #[test]
    fn goal_eval_takes_no_function_name_the_candidate_chose() {
        let suite = "fn easy(x: i64) -> i64 { 100 }\nfn hard(x: i64) -> i64 { 0 }\n@[test]\nfn t() {\n    let s = goal_eval(NAME, 5)\n    assert(s > 50.0)\n}\n";
        assert_eq!(
            judged_on(
                "r10-name",
                &suite.replace("NAME", "\"easy\""),
                "fn solve() -> i64 { 1 }\n"
            ),
            Ok(TestEnd::Completed),
            "control: a literal"
        );
        assert!(
            judged_on(
                "r10-name",
                &suite.replace("NAME", "\"hard\""),
                "fn solve() -> i64 { 1 }\n"
            )
            .is_err(),
            "control: the hard fn fails"
        );
        name_refused(
            "goal_eval",
            &suite.replace("NAME", "name()"),
            "fn name() -> str { \"easy\" }\n",
        );
    }

    #[test]
    fn scheduler_spawn_takes_no_function_name_the_candidate_chose() {
        let suite = "fn grade(x: i64) -> i64 { 9 }\n@[test]\nfn t() {\n    let id = scheduler_spawn(NAME, 0)\n    let n = scheduler_run()\n    assert(scheduler_result(id) == 9)\n}\n";
        assert_eq!(
            judged_on(
                "r10-name",
                &suite.replace("NAME", "\"grade\""),
                "fn solve() -> i64 { 1 }\n"
            ),
            Ok(TestEnd::Completed),
            "control: a literal names the operator's own fn"
        );
        name_refused(
            "scheduler_spawn",
            &suite.replace("NAME", "name()"),
            "fn name() -> str { \"grade\" }\n",
        );
    }

    #[test]
    fn a_goal_constraint_and_a_kernel_goal_take_no_function_name_the_candidate_chose() {
        let pre = "@[adaptive]\nfn metric(x: i64) -> i64 { x }\nfn ok_c(x: i64) -> bool { true }\n";
        let constrained = format!("{pre}@[test]\nfn t() {{\n    let r = goal_run_constrained(\"metric\", CNAME, 10.0, 5)\n    assert(r >= 0.0)\n}}\n");
        assert_eq!(
            judged_on(
                "r10-name",
                &constrained.replace("CNAME", "\"ok_c\""),
                "fn solve() -> i64 { 1 }\n"
            ),
            Ok(TestEnd::Completed),
            "control: a literal constraint"
        );
        name_refused(
            "goal_run_constrained's constraint",
            &constrained.replace("CNAME", "cname()"),
            "fn cname() -> str { \"ok_c\" }\n",
        );
        let kernel = format!("{pre}@[test]\nfn t() {{\n    let p = principal_root(\"r\", true, true, true, 100)\n    let g = kernel_goal_create(p, KNAME, 10.0)\n    let r = kernel_goal_run(g, 3)\n    assert(r >= 0.0)\n}}\n");
        assert_eq!(
            judged_on(
                "r10-name",
                &kernel.replace("KNAME", "\"metric\""),
                "fn solve() -> i64 { 1 }\n"
            ),
            Ok(TestEnd::Completed),
            "control: a literal goal"
        );
        name_refused(
            "kernel_goal_create",
            &kernel.replace("KNAME", "kname()"),
            "fn kname() -> str { \"metric\" }\n",
        );
    }

    /// A candidate's module-level `let` is the candidate's value; the verdict
    /// of one fn's site never decides another's (the key carries the owner).
    #[test]
    fn a_candidates_global_and_a_sibling_fns_site_determine_nothing() {
        r7_refused(
            "a candidate global `let X = 4 as u8`",
            "@[test]\nfn t() {\n    assert(X.ok())\n}\n",
            "let X = 4 as u8\n",
        );
        let suite = "fn check(p: i64) -> bool { p.ok() }\n@[test]\nfn t() {\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(v) => {\n            let f = |p| p.ok()\n            assert(f(v))\n        }\n        None => assert(false)\n    }\n}\n";
        r7_refused(
            "the same receiver text as a sibling fn's pinned one",
            suite,
            "fn solve(d: Dict) { dict_set(d, \"k\", 4 as u8) }\n",
        );
    }

    /// Adjacent (a)/(b): a verdict belongs to its own fn, and a candidate's
    /// methods never steer an operator site.
    #[test]
    fn a_verdict_belongs_to_its_own_fn_and_the_candidate_steers_none() {
        // An unrelated, never-called fn with `|p| p.ok()` does not refuse an
        // honest fully annotated `check(p: i64)`.
        let suite = "fn check(p: i64) -> bool { p.ok() }\nfn unrelated() { let f = |p| p.ok()\n    f(1) }\n@[test]\nfn t() {\n    let r: i64 = solve(3)\n    assert(check(r))\n}\n";
        let out = judged_on("r7", suite, "fn solve(n: i64) -> i64 { n * n }\n");
        assert_eq!(out, Ok(TestEnd::Completed), "control: {out:?}");
        // A candidate method named like an operator method with an open return
        // does not flip the operator's determined `let r = b.pick()`.
        let suite = "type B = { v: i64 }\ntrait Pick {\n    fn pick(self) -> i64\n}\nimpl Pick for B {\n    fn pick(self: B) -> i64 { self.v }\n}\n@[test]\nfn t() {\n    let b = B { v: 9 }\n    let r = b.pick()\n    assert(r.ok())\n}\n";
        let out = judged_on(
            "r7",
            suite,
            "type CB = { w: i64 }\nfn pick<T>(self: CB) -> T { self.w }\n",
        );
        assert_eq!(
            out,
            Ok(TestEnd::Completed),
            "control: a candidate method steers nothing: {out:?}"
        );
    }

    /// B3: a trait name, `dyn`, `Option<Trait>`, `[Trait]` and a fn type
    /// returning one do not constrain the runtime type.
    #[test]
    fn a_trait_annotation_does_not_pin_the_runtime_type() {
        let cand = "fn solve(d: Dict) { dict_set(d, \"k\", 4 as u8) }\n";
        let read = |ann: &str, use_: &str| {
            format!("@[test]\nfn t() {{\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {{\n        Some(v) => {{\n            let y: {ann} = v\n            {use_}\n        }}\n        None => assert(false)\n    }}\n}}\n")
        };
        r7_refused("a trait name", &read("Judge", "assert(y.ok())"), cand);
        r7_refused("dyn Judge", &read("dyn Judge", "assert(y.ok())"), cand);
        let idj = "fn idj(x: Judge) -> Judge { x }\n@[test]\nfn t() {\n    let d = dict_new()\n    solve(d)\n    match dict_get(d, \"k\") {\n        Some(v) => {\n            let y = idj(v)\n            assert(y.ok())\n        }\n        None => assert(false)\n    }\n}\n";
        r7_refused("a trait-typed fn and return", idj, cand);
        r7_refused(
            "Option<Judge>",
            &read(
                "Option<Judge>",
                "match y { Some(z) => assert(z.ok())  None => assert(false) }",
            ),
            "fn solve(d: Dict) { dict_set(d, \"k\", Some(4 as u8)) }\n",
        );
    }

    /// The identifiers called (`name(` / `.name(`) on one line.
    fn called_idents(line: &str) -> Vec<String> {
        let b: Vec<char> = line.chars().collect();
        let mut out = Vec::new();
        let mut i = 0;
        while i < b.len() {
            if b[i].is_alphabetic() || b[i] == '_' {
                let st = i;
                while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                    i += 1;
                }
                // A call on some OTHER receiver (`ch.send(..)`, `x.get(..)`) is not
                // a call of an interpreter fn of that name.
                let pre: String = b[..st].iter().collect();
                let other_receiver =
                    pre.ends_with('.') && !pre.ends_with("self.") && !pre.ends_with("interp.");
                if i < b.len() && b[i] == '(' && !other_receiver {
                    out.push(b[st..i].iter().collect());
                }
            } else {
                i += 1;
            }
        }
        out
    }

    /// `(file, fn name, body)` of every fn item in the interpreter sources
    /// (R51: every `interp/**/*.rs` but `vm/tests.rs`, cut at `mod tests {`).
    fn fn_items() -> Vec<(String, String, String)> {
        let mut out = Vec::new();
        for (file, src) in vm::tests::r51_interp_sources() {
            let lines: Vec<&str> = src.lines().collect();
            let mut i = 0;
            while i < lines.len() {
                let l = lines[i];
                let t = l.trim_start();
                let indent = l.len() - t.len();
                let t2 = t
                    .strip_prefix("pub(super) ")
                    .or_else(|| t.strip_prefix("pub(crate) "))
                    .or_else(|| t.strip_prefix("pub(in crate::interp) "))
                    .or_else(|| t.strip_prefix("pub "))
                    .unwrap_or(t);
                if let Some(rest) = t2.strip_prefix("fn ") {
                    let name: String = rest
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_')
                        .collect();
                    let mut body = String::new();
                    let mut j = i + 1;
                    // The signature may span lines; the body ends at the line
                    // that closes at the fn's own indent.
                    while j < lines.len() {
                        let lj = lines[j];
                        if lj.len() - lj.trim_start().len() == indent
                            && lj.trim_start().starts_with('}')
                        {
                            break;
                        }
                        body.push_str(lj);
                        body.push('\n');
                        j += 1;
                    }
                    out.push((file.clone(), name, body));
                }
                i += 1;
            }
        }
        out
    }

    /// Every interpreter fn that runs a `Value::Closure` or an `FnDef`, found
    /// by closure over the call graph from the primitives that do. The
    /// evaluator (`eval.rs`), the bytecode engine (`vm/`, R50: the
    /// evaluator's twin) and the builtin dispatcher are the graph's ROOTS, not
    /// its nodes: everything reaches them.
    fn user_code_runners() -> std::collections::HashSet<String> {
        // R51: main's call path (AX-54 split `call_fn_frame`; `call_mut_with`
        // is v0.22's `call_fn_mut`; AX-40 added `call_closure_arg`) and the
        // VM entries a builtin arm calls directly (`fold_leaf`, T1d); R51 S2.3
        // made the general fn frame and the closure frame const-generic
        // (`call_fn_frame`, `call_closure_frame`).
        const PRIMITIVES: &[&str] = &[
            "call_fn",
            "call_fn_entry",
            "call_fn_in",
            "call_fn_common",
            "call_fn_in_general",
            "call_fn_frame",
            "call_test_fn",
            "call_mut_with",
            "call_closure",
            "call_closure_arg",
            "call_local_closure",
            "call_closure_owned_by",
            "call_closure_frame",
            "run_body",
            "run_test_body",
            "run_lambda",
            "fold_leaf",
        ];
        const ROOTS: &[&str] = &[
            "eval",
            "eval_call",
            "eval_block",
            "eval_binop",
            "call_builtin",
            "run_handler_arm",
            "replay_continuation",
            // Host entry points that START a run: no builtin arm reaches them
            // (and `axon_host_await` is also the name of an extern declaration).
            "run_program_inner",
            "run_main",
            "run_test_fn_inner",
            "axon_host_await",
            "run_named_fn_as_bool_with_score",
            "run_suspendable_values_inner",
            "run_property_test_inner",
        ];
        let items = fn_items();
        let mut runners: std::collections::HashSet<String> =
            PRIMITIVES.iter().map(|s| s.to_string()).collect();
        loop {
            let before = runners.len();
            for (file, name, body) in &items {
                if file == "eval.rs" || file.starts_with("vm/") || ROOTS.contains(&name.as_str()) {
                    continue;
                }
                if body.lines().any(|l| {
                    !l.trim_start().starts_with("//")
                        && called_idents(l)
                            .iter()
                            .any(|c| runners.contains(c) && c != name)
                }) {
                    runners.insert(name.clone());
                }
            }
            if runners.len() == before {
                break;
            }
        }
        runners
    }

    /// What the sweep established about a builtin that runs user code.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Disp {
        /// The result's type names a type variable (`[U]`, `T`): the pin
        /// analysis treats it as undetermined.
        Open,
        /// A `Dict`: its values are an untyped position (amendment 83).
        Dict,
        /// A scalar the builtin builds itself (a count, a score, `()`).
        Scalar,
        /// The candidate fn's value is cast to the (closed, scalar) declared
        /// type at the seal crossing in the arm itself (R51 S3 ports the cast:
        /// `a_crossing_builtin_casts_a_sealed_fns_result_in_its_arm`).
        Crossing,
    }

    /// `call_builtin`'s arms in `interp/builtins.rs`, split at their 12-space
    /// `"name" … =>` headers: (the arm's names, its body).
    pub(super) fn builtin_arms(src: &str) -> Vec<(Vec<String>, String)> {
        let mut arms: Vec<(Vec<String>, String)> = Vec::new();
        for line in src.lines() {
            let indent = line.len() - line.trim_start().len();
            let t = line.trim_start();
            if indent == 12 && t.starts_with('"') && t.contains("=>") {
                let head = &t[..t.find("=>").unwrap()];
                let names = head
                    .split('|')
                    .map(|n| n.trim().trim_matches('"').to_string())
                    .collect();
                arms.push((names, String::new()));
            } else if let Some(last) = arms.last_mut() {
                last.1.push_str(line);
                last.1.push('\n');
            }
        }
        // R51: the sweep matched the real dispatcher, not an empty cut (295
        // arms at S5).
        assert!(arms.len() >= 250, "only {} builtin arms found", arms.len());
        arms
    }

    /// v0.22 (C9 round 9, amendment 96): a `Disp::Crossing` builtin casts the
    /// sealed fn's result to its declared type in the arm itself. The
    /// classification half of v0.22's drift test runs now
    /// (`every_builtin_that_runs_user_code_is_classified`); this half needs
    /// the cast S3 ports.
    #[test]
    fn a_crossing_builtin_casts_a_sealed_fns_result_in_its_arm() {
        let sources = vm::tests::r51_interp_sources();
        let src = &sources
            .iter()
            .find(|(f, _)| f == "builtins.rs")
            .expect("interp/builtins.rs")
            .1;
        let arms = builtin_arms(src);
        let name = "sandbox_run";
        let (_, body) = arms
            .iter()
            .find(|(ns, _)| ns.iter().any(|n| n == name))
            .unwrap_or_else(|| panic!("no `{name}` arm"));
        assert!(
            body.contains("self.fn_is_sealed(f)"),
            "DRIFT: `{name}` is classified Crossing but its arm casts no sealed fn's result"
        );
    }

    /// C9 round 9 drift: every builtin that RUNS user code (a callback, a fn
    /// named by string, a goal metric, a scheduler fiber) is classified by how
    /// its result reaches operator code. A new one fails this test until it is
    /// classified, and its declared return must be consistent with the class.
    #[test]
    fn every_builtin_that_runs_user_code_is_classified() {
        use Disp::*;
        const SCALAR_RETS: &[&str] = &["i64", "f64", "bool", "str", "()", "Result<i64, str>"];
        const TABLE: &[(&str, Disp)] = &[
            ("arr_all", Scalar),
            ("arr_any", Scalar),
            ("arr_count_if", Scalar),
            ("arr_drop_while", Open),
            ("arr_filter", Open),
            ("arr_find", Open),
            ("arr_fold", Open),
            ("arr_group_by", Dict),
            ("arr_map", Open),
            ("arr_max_by", Open),
            ("arr_min_by", Open),
            ("arr_partition", Open),
            ("arr_sort_by", Open),
            ("arr_sum_by", Scalar),
            ("arr_sum_by_f64", Scalar),
            ("arr_take_while", Open),
            ("arr_zip_with", Open),
            ("dict_each", Scalar),
            ("dict_filter", Dict),
            ("dict_map_values", Dict),
            ("goal_continue", Scalar),
            ("goal_eval", Scalar),
            ("goal_run", Scalar),
            ("goal_run_categorical", Scalar),
            ("goal_run_constrained", Scalar),
            ("goal_run_multistart", Scalar),
            ("goal_run_random", Scalar),
            ("http_sse", Scalar),
            ("http_sse_post", Scalar),
            ("kernel_goal_run", Scalar),
            ("sandbox_run", Crossing),
            ("scheduler_run", Scalar),
            ("supervisor_run", Scalar),
        ];
        let sources = vm::tests::r51_interp_sources();
        let src = &sources
            .iter()
            .find(|(f, _)| f == "builtins.rs")
            .expect("interp/builtins.rs")
            .1;
        let arms = builtin_arms(src);
        // C9 round 10: not four literal call patterns. A builtin runs user code
        // if its arm calls ANY fn that (transitively, through the interpreter's
        // own fns) reaches a runner of a `Value::Closure` or an `FnDef`.
        let runners = user_code_runners();
        let runs_user_code = |body: &str| {
            body.lines().any(|l| {
                let l = l.trim_start();
                !l.starts_with("//") && called_idents(l).iter().any(|c| runners.contains(c))
            })
        };
        let declared: std::collections::HashMap<&str, &str> = crate::builtins::BUILTINS
            .iter()
            .map(|b| (b.name, b.ret))
            .collect();
        let mut found: Vec<String> = Vec::new();
        for (names, body) in &arms {
            if !runs_user_code(body) {
                continue;
            }
            for n in names {
                if declared.contains_key(n.as_str()) {
                    found.push(n.clone());
                }
            }
        }
        found.sort();
        found.dedup();
        let mut want: Vec<String> = TABLE.iter().map(|(n, _)| n.to_string()).collect();
        want.sort();
        assert_eq!(
            found, want,
            "DRIFT: a builtin arm that runs user code is unclassified (or a classified one no longer does); decide how its result reaches operator code and list it"
        );
        for (name, disp) in TABLE {
            let ret = declared[name];
            let ok = match disp {
                Open => pin::builtin_ret_open(ret),
                Dict => ret == "Dict",
                Scalar => SCALAR_RETS.contains(&ret),
                Crossing => SCALAR_RETS.contains(&ret),
            };
            assert!(
                ok,
                "DRIFT: `{name}` is classified {disp:?} but declares `-> {ret}`"
            );
        }
        // The goal metric and the property runner score or discard the result.
        let goal = &sources
            .iter()
            .find(|(f, _)| f == "goal.rs")
            .expect("interp/goal.rs")
            .1;
        let lines: Vec<&str> = goal.lines().collect();
        let mut calls = 0;
        for (i, l) in lines.iter().enumerate() {
            if l.trim_start().starts_with("//") || !l.contains("self.call_fn(") {
                continue;
            }
            calls += 1;
            let window = lines[i..(i + 40).min(lines.len())].join("\n");
            assert!(
                window.contains("numeric_score(") || window.contains("Value::Bool("),
                "DRIFT: goal.rs line {} runs user code and does not reduce the result to a score or a bool",
                i + 1
            );
        }
        assert!(calls >= 5, "only {calls} goal.rs call_fn sites found");
    }
    include!("interp/tests_v022_s4.rs");
    include!("interp/tests_v022_a.rs");
    include!("interp/tests_v022_b.rs");
    include!("interp/tests_v022_c.rs");
}

#[cfg(test)]
mod literal_escape_tests {
    use super::*;

    /// M1. A value containing `{` used to be dumped un-escaped, and since Axon
    /// strings interpolate, the dumped `let j = "{"` was itself unlexable. Every
    /// subsequent cell then died on `unclosed \`{\` in interpolated string` — the
    /// session was permanently bricked with no recovery path, and a model that
    /// builds a JSON-ish string does this immediately.
    #[test]
    fn braces_in_a_dumped_string_are_escaped_for_re_parsing() {
        let v = Value::Str(Rc::new("{\"a\": 1}".to_string()));
        let lit = value_as_literal(&v).expect("a string always has a literal form");
        assert!(
            lit.contains("{{") && lit.contains("}}"),
            "both braces must be doubled for the interpolating lexer: {lit}"
        );
    }

    /// M2. Option/Result/Tuple/Enum all HAVE literal syntax, and a session binds
    /// them constantly — `let o = parse_int(s)` is a `Result`. They used to fall
    /// into the catch-all and be reported as unserialisable, so the most
    /// ordinary binding a model writes could not cross a cell boundary.
    #[test]
    fn option_result_tuple_and_enum_have_literal_forms() {
        let cases: Vec<(Value, &str)> = vec![
            (Value::Some(VBox::new(Value::Int(2))), "Some(2)"),
            (Value::None, "None"),
            (Value::Ok(VBox::new(Value::Int(5))), "Ok(5)"),
            (
                Value::Err(VBox::new(Value::Str(Rc::new("bad".to_string())))),
                "Err(\"bad\")",
            ),
            (Value::tuple(vec![Value::Int(1), Value::Int(2)]), "(1, 2)"),
            // A 1-tuple needs the trailing comma, or it re-parses as a
            // parenthesised expression and silently changes type.
            (Value::tuple(vec![Value::Int(7)]), "(7,)"),
        ];
        for (v, want) in cases {
            assert_eq!(
                value_as_literal(&v).expect("must have a literal form"),
                want
            );
        }
        // A fieldless enum variant renders bare; a fielded one renders with
        // sorted fields, as structs do.
        let e = Value::Enum(Rc::new(EnumVal {
            enum_name: intern("Shape"),
            variant: intern("Circle"),
            fields: Fields::from_pairs([(intern("r"), Value::Float(2.0))]),
        }));
        assert_eq!(value_as_literal(&e).unwrap(), "Shape::Circle { r: 2.0 }");
        let bare = Value::Enum(Rc::new(EnumVal {
            enum_name: intern("Shape"),
            variant: intern("Point"),
            fields: Fields::default(),
        }));
        assert_eq!(value_as_literal(&bare).unwrap(), "Shape::Point");
    }

    /// The round trip is the real requirement: what comes back must equal what
    /// went in. Escaping that is not the parser's inverse would corrupt values
    /// silently, which is worse than the wedge it replaces.
    #[test]
    fn a_braced_string_round_trips_through_the_parser() {
        for original in [
            "{",
            "}",
            "{\"k\": [1, 2]}",
            "a{b}c",
            "{{already doubled}}",
            "quote\" and \\ and \ttab",
        ] {
            let lit = value_as_literal(&Value::Str(Rc::new(original.to_string()))).unwrap();
            let src = format!("let x = {lit}\nfn main() -> i64 {{ 0 }}\n");
            let prog = crate::parse_source(&src)
                .unwrap_or_else(|e| panic!("dumped literal must re-parse ({original:?}): {e}"));
            let mut interp = Interp::build(&prog).expect("unsealed build");
            interp.init_globals().expect("globals initialise");
            let got = interp
                .globals
                .get(&intern("x"))
                .expect("let x must be bound");
            // Value has no PartialEq; compare the rendered form, which is what
            // the session actually round-trips anyway.
            let got_s = match got {
                Value::Str(t) => t.as_str(),
                other => panic!("expected a Str, got {other:?}"),
            };
            assert_eq!(
                got_s, original,
                "round trip changed the value: {original:?} -> {lit}"
            );
        }
    }
}

#[cfg(test)]
mod value_shape_leak_tests {
    //! `value_shape` is the source of the RLM shape inventory, whose whole
    //! contract is that it carries STRUCTURE and no dataset values. A dict is
    //! the one container whose keys can be data rather than schema.
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    fn dict(pairs: &[(&str, Value)]) -> Value {
        let mut m = BTreeMap::new();
        for (k, v) in pairs {
            m.insert((*k).to_string(), v.clone());
        }
        Value::Dict(Rc::new(RefCell::new(m.into())))
    }

    /// A dict built by GROUPING is keyed by the data. Printing that key set put
    /// `north`/`south`/`east` straight into the inventory and voided two arm-A
    /// measurement runs (the harness leak guard caught it and withheld the arm).
    #[test]
    fn grouping_dict_keys_are_not_emitted() {
        let s = value_shape(&dict(&[
            ("north", Value::Int(315)),
            ("south", Value::Int(95)),
        ]));
        for leaked in ["north", "south", "315", "95"] {
            assert!(
                !s.contains(leaked),
                "value_shape leaked the dataset value `{leaked}`: {s}"
            );
        }
    }

    /// The shape must still be USEFUL: length, key type, and the value shape are
    /// what tell a model whether `dict_get` hands back an i64 or a record.
    #[test]
    fn dict_shape_still_carries_length_and_value_type() {
        let s = value_shape(&dict(&[("north", Value::Int(315))]));
        assert!(s.contains("len=1"), "{s}");
        assert!(s.contains("keys are str"), "{s}");
        assert!(s.contains("i64"), "{s}");
    }
}

/// AX-46/AX-47: records store `Sym`-keyed fields in the definition's order
/// behind an `Rc`. None of that may be observable: printing orders fields by
/// name, equality ignores construction order, enum variants match by
/// interned name, declared narrow fields are coerced, and a write through a
/// shared record copies it first.
#[cfg(test)]
mod record_layout_tests {
    use super::*;

    fn out(src: &str) -> (i32, String) {
        let program = crate::parse_source(src).expect("parse failed");
        run_program_capturing(&program)
    }

    const RECORDS: &str = r#"
type P = { z: i64, a: i64, m: str }
type B = { lo: u8, hi: i16 }
type S = Circle { r: f64 } | Rect { w: i64, h: i64 } | Empty

fn area(s: S) -> i64 {
    match s {
        S::Rect { w, h } => w * h,
        S::Circle { r } => f64_to_i64(r),
        S::Empty => 0,
    }
}
"#;

    fn run_main(body: &str) -> (i32, String) {
        out(&format!("{RECORDS}\nfn main() {{\n{body}\n}}\n"))
    }

    #[test]
    fn fields_print_in_name_order_whatever_the_literal_order() {
        let (code, s) = run_main(
            r#"let p = P { m: "hi", a: 2, z: 1 }
               println("{p}")
               println("{S::Rect { h: 3, w: 4 }}")
               println("{[B { hi: 300, lo: 250 }]}")"#,
        );
        assert_eq!(code, 0, "{s}");
        assert_eq!(
            s,
            "P { a: 2, m: hi, z: 1 }\nS::Rect { h: 3, w: 4 }\n[B { hi: 300, lo: 250 }]\n"
        );
    }

    #[test]
    fn equality_ignores_construction_order() {
        let (code, s) = run_main(
            r#"println(to_str(P { m: "hi", a: 2, z: 1 } == P { z: 1, a: 2, m: "hi" }))
               println(to_str(P { m: "hi", a: 2, z: 1 } != P { a: 2, z: 3, m: "hi" }))
               println(to_str(S::Rect { w: 4, h: 3 } == S::Rect { h: 3, w: 4 }))
               println(to_str(S::Rect { w: 4, h: 3 } == S::Rect { h: 4, w: 3 }))
               println(to_str(S::Empty == S::Empty))"#,
        );
        assert_eq!(code, 0, "{s}");
        assert_eq!(s, "true\ntrue\ntrue\nfalse\ntrue\n");
    }

    #[test]
    fn enum_variants_match_by_interned_name() {
        let (code, s) = run_main(
            r#"println(to_str(area(S::Rect { h: 3, w: 4 })))
               println(to_str(area(S::Circle { r: 2.5 })))
               println(to_str(area(S::Empty)))"#,
        );
        assert_eq!(code, 0, "{s}");
        assert_eq!(s, "12\n2\n0\n");
    }

    #[test]
    fn narrow_fields_are_coerced_at_construction() {
        // `lo` is declared `u8`: arithmetic on it is u8-checked, so 250 + 10
        // must overflow rather than print 260.
        let (code, s) = run_main(
            r#"let b = B { hi: 300, lo: 250 }
               let s = b.lo + as_u8(10)
               println("{s}")"#,
        );
        assert_eq!(code, RUNTIME_PANIC_EXIT_CODE, "{s}");
        assert_eq!(s, "");
    }

    #[test]
    fn a_field_write_through_a_shared_record_copies_it() {
        let (code, s) = run_main(
            r#"let bs = [B { hi: 300, lo: 250 }]
               let c = bs[0]
               c.lo = as_u8(9)
               println("{bs}")
               println("{c}")
               let ps = [P { z: 1, a: 2, m: "hi" }]
               let k = ps[0]
               k.a = 99
               println(to_str(ps[0].a))
               println(to_str(k.a))"#,
        );
        assert_eq!(code, 0, "{s}");
        assert_eq!(s, "[B { hi: 300, lo: 250 }]\nB { hi: 300, lo: 9 }\n2\n99\n");
    }

    #[test]
    fn closures_capture_records() {
        let (code, s) = run_main(
            r#"let p = P { m: "hi", a: 2, z: 1 }
               let f = |n: i64| n + p.a + p.z
               let g = |n: i64| P { z: n, a: p.a, m: p.m }
               println(to_str(f(10)))
               println("{g(7)}")
               p.a = 40
               println(to_str(f(10)))
               println("{p}")"#,
        );
        assert_eq!(code, 0, "{s}");
        assert_eq!(
            s,
            "13\nP { a: 2, m: hi, z: 7 }\n13\nP { a: 40, m: hi, z: 1 }\n"
        );
    }
}
