//! R51 S3: the declared-type casts, closure and channel contracts and the
//! operator's dict snapshots (`interp/conform.rs`), unit by unit on main's
//! value model. Until S3c wires the seal edges that call them, these tests
//! drive each entry point directly on an `Interp` built from a small suite:
//! an unsealed one for the casts (a cast does not consult the seal), a sealed
//! one ([`Interp::build_with`] with a sealed directory, as `taint_tests` does)
//! for the edges whose rule depends on which side runs.

use super::conform::{Cx, MAX_CAST_DEPTH};
use super::*;
use crate::ast::AxonType;

const SUITE: &str = "\
trait Same {
    fn me(self) -> Self
}
trait Shape {
    fn area(self) -> i64
}
type Sq = { s: i64 }
type Fake = { s: i64 }
type Wrap<T> = { v: T }
enum Pick { Nothing, One { x: i64 }, }
type Pos = i64 where _ > 0
impl Shape for Sq {
    fn area(self: Sq) -> i64 {
        self.s
    }
}
impl Same for Sq {
    fn me(self) -> Self {
        self
    }
}
fn same<T>(a: T, b: T) -> T {
    a
}
fn area_of<T: Shape>(x: T) -> i64 {
    x.area()
}
fn take<T>(c: Chan<T>) -> T {
    c.recv()
}
fn sq() -> Sq {
    Sq { s: 3 }
}
fn fake() -> Fake {
    Fake { s: 3 }
}
fn wrap_i() -> Wrap<i64> {
    Wrap { v: 1 }
}
fn wrap_s() -> Wrap<str> {
    Wrap { v: \"s\" }
}
fn one() -> Pick {
    Pick::One { x: 1 }
}
fn adder() -> fn(i64) -> i64 {
    let k = 2
    |x: i64| x + k
}
fn untyped() -> fn(i64) -> i64 {
    let k = 2
    |x| x + k
}
fn ch_len(c: i64, q: Chan<i64>) -> i64 {
    let n = 0
    if c == 1 { n = q.len() }
    n
}
fn ch_recv(c: i64, q: Chan<i64>) -> i64 {
    q.recv()
}
fn ch_count(c: i64, q: Chan<i64>) -> i64 {
    q.len()
}
fn ch_select(c: i64, q: Chan<i64>) -> i64 {
    let x = 0
    if c == 1 { select { q.recv() => x = 1 } }
    x
}
fn ch_pick(c: i64, q: Chan<i64>, r: Chan<i64>) -> i64 {
    let p = if c == 1 { q } else { r }
    p.len()
}
";

const CAND: &str = "fn solve() -> i64 { 41 }\n";

/// The suite (and, for `sealed`, a sealed candidate beside it).
fn prog(tag: &str) -> (Program, std::path::PathBuf) {
    use crate::span::intern_source;
    let sdir = format!("/{tag}-sealed");
    let s = crate::parse_source_in(SUITE, intern_source(&format!("/{tag}-suite/h.ax"), SUITE))
        .expect("suite parses");
    let c = crate::parse_source_in(CAND, intern_source(&format!("{sdir}/f.ax"), CAND))
        .expect("candidate parses");
    let prog = Program {
        items: s.items.into_iter().chain(c.items).collect(),
    };
    (prog, std::path::PathBuf::from(sdir))
}

/// An `Interp` over the suite; `sealed` gives it a live seal (a unit-test
/// `Interp`, built without a run entry).
fn with_ix<R>(tag: &str, sealed: bool, f: impl FnOnce(&Interp<'_>) -> R) -> R {
    let (prog, sdir) = prog(tag);
    let dirs = if sealed { vec![sdir] } else { Vec::new() };
    let ix = Interp::build_with(&prog, &dirs, Engine::Tree);
    assert_eq!(ix.seal.active, sealed);
    f(&ix)
}

/// The value the zero-argument suite fn `name` returns.
fn run(ix: &Interp<'_>, name: &str) -> Value {
    let f = *ix.fns.get(name).expect("a fn of the suite");
    let mut env = ix.new_env();
    let r = if ix.seal.active {
        ix.eval_t::<true>(&f.body, &mut env)
    } else {
        ix.eval_t::<false>(&f.body, &mut env)
    };
    match r {
        Ok(v) | Err(Flow::Return(v)) => v,
        Err(e) => panic!("{name}: {e:?}"),
    }
}

/// The type `s` spells; `?` (the unstated position, which no source text
/// spells) is written as itself.
fn ty(s: &str) -> AxonType {
    let t = crate::parser::parse_type_text(&s.replace('?', "Unstated"))
        .unwrap_or_else(|| panic!("`{s}` parses as a type"));
    unstated(t)
}

fn unstated(t: AxonType) -> AxonType {
    use AxonType as A;
    let b = |x: Box<A>| Box::new(unstated(*x));
    let v = |xs: Vec<A>| xs.into_iter().map(unstated).collect();
    match t {
        A::Named(n) if n == "Unstated" => A::Named("?".into()),
        A::Named(_) | A::TypeParam(_) | A::DynTrait(_) => t,
        A::Generic { base, args } => A::Generic {
            base,
            args: v(args),
        },
        A::Fn { params, ret } => A::Fn {
            params: v(params),
            ret: b(ret),
        },
        A::Result { ok, err } => A::Result {
            ok: b(ok),
            err: b(err),
        },
        A::Option(x) => A::Option(b(x)),
        A::Chan(x) => A::Chan(b(x)),
        A::Slice(x) => A::Slice(b(x)),
        A::Ref(x) => A::Ref(b(x)),
        A::RefMut(x) => A::RefMut(b(x)),
        A::RawPtr(x) => A::RawPtr(b(x)),
        A::Tuple(xs) => A::Tuple(v(xs)),
        A::Union(xs) => A::Union(v(xs)),
    }
}

fn cast(ix: &Interp<'_>, v: &Value, t: &str) -> Result<Value, String> {
    let mut v = v.clone();
    ix.cast(&mut v, &ty(t), &Cx::default()).map(|()| v)
}

fn refused<T: std::fmt::Debug>(r: &Result<T, Flow>, needle: &str) -> bool {
    matches!(r, Err(Flow::Panic(m)) if m.contains(needle))
}

fn strv(s: &str) -> Value {
    Value::Str(Rc::new(s.to_string()))
}

fn rt(t: &AxonType) -> String {
    crate::doc::render_type(t)
}

fn rts(ts: &[AxonType]) -> Vec<String> {
    ts.iter().map(rt).collect()
}

fn record(name: &str, fields: &[(&str, Value)]) -> Value {
    Value::Struct(Rc::new(StructVal {
        name: intern(name),
        fields: Fields::from_pairs(fields.iter().map(|(k, v)| (intern(k), v.clone()))),
    }))
}

fn soft(name: &str, inner: Value) -> Value {
    record(name, &[("value", inner), ("confidence", Value::Float(0.5))])
}

fn dict(pairs: &[(&str, Value)]) -> Rc<RefCell<std::collections::BTreeMap<String, Value>>> {
    Rc::new(RefCell::new(
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.clone()))
            .collect(),
    ))
}

fn chan() -> Rc<RefCell<std::collections::VecDeque<Value>>> {
    Rc::new(RefCell::new(std::collections::VecDeque::new()))
}

fn closure_of(v: &Value) -> &Rc<ClosureVal> {
    match v {
        Value::Closure(cv) => cv,
        other => panic!("expected a closure, got {other:?}"),
    }
}

/// A capture-free one-parameter closure (`|x| x`).
fn id_closure() -> Value {
    Value::Closure(Rc::new(ClosureVal::plain(
        Rc::new(ClosureCode::unresolved(
            Box::new([intern("x")]),
            Expr::Ident("x".into()),
        )),
        Captures::default(),
    )))
}

fn cx_of(ix: &Interp<'_>, name: &str) -> Cx {
    Cx::of_fn(ix.fns.get(name).expect("a fn of the suite"), None)
}

// -- representation A ----------------------------------------------------------

/// Spec Fork 3 / TaintPort-5: the per-reference contract lives on the
/// `ClosureVal`, and a cast allocates a NEW one that shares the capture cell,
/// so the value stays one word wide.
#[test]
fn value_is_still_sixteen_bytes() {
    assert_eq!(std::mem::size_of::<Value>(), 16);
}

/// A cast to `fn(..) -> ..` wraps the closure in a new `ClosureVal` that
/// SHARES the capture cell (one identity, `addr_of`), carries the contract
/// for that reference only, and leaves the uncast reference contract-free.
#[test]
fn a_cast_closure_shares_its_capture_cell_and_carries_the_contract_per_reference() {
    with_ix("r51-s3-repa", false, |ix| {
        let orig = run(ix, "untyped");
        let cast_v = cast(ix, &orig, "fn(i64) -> i64").expect("conforms");
        let (a, b) = (closure_of(&orig), closure_of(&cast_v));
        assert!(!Rc::ptr_eq(a, b), "the cast made a new reference");
        assert!(
            Rc::ptr_eq(&a.captured, &b.captured),
            "ATTACK: the cast copied the capture cell - the two references no longer share state"
        );
        assert_eq!(
            taint::addr_of(&orig),
            taint::addr_of(&cast_v),
            "ATTACK: one closure, two identities - a taint keyed by one misses the other"
        );
        assert!(a.contract.is_none(), "the uncast reference crossed nothing");
        assert!(
            b.contract.is_some(),
            "the cast reference carries its contract"
        );
        assert_eq!(
            rts(&Interp::param_types(&b.contract, 0)),
            ["i64"],
            "the contract states the declared parameter"
        );
        assert_eq!(b.seal, a.seal, "the marks travel with the cell");
        // Crossing the same closed type again is not re-wrapped.
        let again = cast(ix, &cast_v, "fn(i64) -> i64").expect("conforms");
        assert!(Rc::ptr_eq(closure_of(&again), b), "not re-wrapped");
        // A second, different type chains outside the first.
        let twice = cast(ix, &cast_v, "fn(i64) -> ?").expect("conforms");
        assert_eq!(
            Interp::param_types(&closure_of(&twice).contract, 0).len(),
            2
        );
    });
}

/// AX-40's lend test on representation A: a cast's wrapper is a second way to
/// reach the cell, so the cell is lent only while one `ClosureVal` holds it.
#[test]
fn the_lend_test_counts_the_capture_cells_holders() {
    with_ix("r51-s3-lend", false, |ix| {
        let orig = run(ix, "untyped");
        let a = closure_of(&orig);
        assert!(cell_is_private(a, 1), "a closure held once lends its cell");
        assert!(
            !cell_is_private(a, 2),
            "a reference the caller did not count"
        );
        let wrapped = cast(ix, &orig, "fn(i64) -> i64").expect("conforms");
        assert!(
            !cell_is_private(a, 1),
            "ATTACK: the cell was lent while a cast's wrapper could still reach it"
        );
        assert!(!cell_is_private(closure_of(&wrapped), 1));
        drop(wrapped);
        assert!(
            cell_is_private(a, 1),
            "private again once the wrapper is gone"
        );
    });
}

/// TaintPort-15: the seal marks are typed fields, never capture entries (a
/// capture entry would shift `param_base`). In a sealed run a closure made in
/// a SEALED frame is marked, with its parameter annotations as its first
/// contract; one made in an operator frame is not marked.
#[test]
fn a_sealed_frames_closure_is_marked_by_a_typed_field_not_a_capture_entry() {
    with_ix("r51-s3-mark", true, |ix| {
        let op = run(ix, "adder");
        let cand = ix.with_frame(true, || run(ix, "adder"));
        let (o, c) = (closure_of(&op), closure_of(&cand));
        assert!(!o.seal.is_candidates(), "an operator frame's closure");
        assert!(
            c.seal.sealed_origin && c.seal.is_candidates(),
            "ATTACK: a closure a sealed frame made carries no mark"
        );
        assert_eq!(c.captured.borrow().len(), 1, "only `k` is captured");
        assert_eq!(o.captured.borrow().len(), 1);
        assert_eq!(
            rts(&Interp::param_types(&c.contract, 0)),
            ["i64"],
            "its own annotation is its first contract"
        );
        // A fn VALUE a sealed frame takes is marked too (it may take only its
        // own fns: the call edge applies to a fn named in value position, R51
        // S3); an operator's is not marked.
        let g = ix
            .with_frame(true, || ix.ident_unbound("solve", intern("solve")))
            .expect("a fn value");
        assert!(closure_of(&g).seal.sealed_fnval);
        let h = ix.ident_unbound("sq", intern("sq")).expect("a fn value");
        assert!(!closure_of(&h).seal.is_candidates());
    });
    // Unsealed: no marks and no contract (the hot path builds nothing).
    with_ix("r51-s3-mark-u", false, |ix| {
        let c = run(ix, "adder");
        assert!(closure_of(&c).contract.is_none());
        assert!(!closure_of(&c).seal.is_candidates());
    });
}

/// Spec Fork 3: the reference-semantic objects - the only values an alias can
/// WRITE through - are a dict, a channel and a closure's capture cell, and
/// `addr_of` (the identity every address-keyed taint table uses) covers
/// exactly those. The match below has no wildcard: a new `Value` variant
/// does not compile until it is classified here.
#[test]
fn addr_of_covers_exactly_the_reference_semantic_objects() {
    fn reference_semantic(v: &Value) -> bool {
        match v {
            Value::Dict(_) | Value::Chan(_) | Value::Closure(_) => true,
            Value::Int(_)
            | Value::SizedInt { .. }
            | Value::Float(_)
            | Value::Decimal(_)
            | Value::Bool(_)
            | Value::Str(_)
            | Value::Unit
            | Value::Array(_)
            | Value::Struct(_)
            | Value::Enum(_)
            | Value::Some(_)
            | Value::None
            | Value::Ok(_)
            | Value::Err(_)
            | Value::Tuple(_)
            | Value::Handle(_) => false,
        }
    }
    let one = || Box::new(Value::Int(1));
    let samples = [
        Value::Int(1),
        Value::SizedInt {
            val: 1,
            ty: IntWidth::U8,
        },
        Value::Float(1.0),
        Value::decimal(1),
        Value::Bool(true),
        strv("s"),
        Value::Unit,
        Value::Array(Rc::new(vec![Value::Int(1)])),
        record("Sq", &[("s", Value::Int(1))]),
        Value::Enum(Rc::new(EnumVal {
            enum_name: intern("Pick"),
            variant: intern("Nothing"),
            fields: Fields::from_pairs([]),
        })),
        Value::Some(one()),
        Value::None,
        Value::Ok(one()),
        Value::Err(one()),
        Value::tuple(vec![Value::Int(1), Value::Int(2)]),
        Value::Handle(Rc::new(HandleVal {
            module: "m".into(),
            name: "h".into(),
            payload: 0,
            resource: false,
        })),
        Value::Dict(dict(&[])),
        Value::Chan(chan()),
        id_closure(),
    ];
    let mut kinds = std::collections::HashSet::new();
    for v in &samples {
        kinds.insert(std::mem::discriminant(v));
        assert_eq!(
            taint::addr_of(v).is_some(),
            reference_semantic(v),
            "DRIFT: addr_of disagrees with the reference-semantic set on {v:?}"
        );
    }
    // One sample per variant (the exhaustive match above lists 19).
    assert_eq!(kinds.len(), 19, "DRIFT: give every Value variant a sample");
    // The closure's identity is its capture cell, not the `ClosureVal`.
    let c = id_closure();
    let Value::Closure(cv) = &c else {
        unreachable!()
    };
    assert_eq!(
        taint::addr_of(&c),
        Some(Rc::as_ptr(&cv.captured) as *const u8 as usize)
    );
}

// -- casts -------------------------------------------------------------------------

/// Integers by WIDTH (the key a method call dispatches on): an `i64` at a
/// fixed width is converted, another width refused, never waved through.
#[test]
fn an_integer_is_cast_by_width_not_kind() {
    with_ix("r51-s3-int", false, |ix| {
        assert_eq!(
            cast(ix, &Value::Int(5), "u8").map(|v| matches!(
                v,
                Value::SizedInt {
                    val: 5,
                    ty: IntWidth::U8
                }
            )),
            Ok(true)
        );
        let w = Value::SizedInt {
            val: 5,
            ty: IntWidth::U16,
        };
        assert!(cast(ix, &w, "u16").is_ok());
        assert!(cast(ix, &w, "u8").is_err(), "ATTACK: a u16 passed as a u8");
        assert!(
            cast(ix, &w, "i64").is_err(),
            "ATTACK: a u16 passed as an i64"
        );
        assert!(cast(ix, &Value::Float(1.0), "i64").is_err());
        assert!(cast(ix, &strv("5"), "i64").is_err());
        assert!(cast(ix, &Value::Bool(true), "bool").is_ok());
        assert!(cast(ix, &Value::Unit, "()").is_ok());
        assert!(cast(ix, &Value::Int(1), "Dict").is_err());
        assert!(
            cast(ix, &Value::Int(1), "?").is_ok(),
            "an unstated position"
        );
        assert!(
            cast(ix, &Value::Int(1), "NoSuchType").is_ok(),
            "an unknown name"
        );
    });
}

/// A record by NAME, then its fields (the C9 round-4 attack: the
/// candidate's look-alike `Fake` where the operator declared `Sq`).
#[test]
fn a_record_is_cast_by_name_then_by_field() {
    with_ix("r51-s3-rec", false, |ix| {
        let sq = run(ix, "sq");
        assert!(cast(ix, &sq, "Sq").is_ok());
        assert!(
            cast(ix, &run(ix, "fake"), "Sq").is_err(),
            "ATTACK: a look-alike record passed by shape"
        );
        let bad = record("Sq", &[("s", strv("x"))]);
        let e = cast(ix, &bad, "Sq").expect_err("a mistyped field");
        assert!(e.contains("field `s` of `Sq`"), "{e}");
        assert!(cast(ix, &Value::Int(1), "Sq").is_err());
        // Enums: name, variant and fields.
        let one = run(ix, "one");
        assert!(cast(ix, &one, "Pick").is_ok());
        assert!(cast(ix, &one, "Sq").is_err());
        let bad = Value::Enum(Rc::new(EnumVal {
            enum_name: intern("Pick"),
            variant: intern("One"),
            fields: Fields::from_pairs([(intern("x"), strv("s"))]),
        }));
        assert!(
            cast(ix, &bad, "Pick").is_err(),
            "ATTACK: a mistyped variant field"
        );
        let ghost = Value::Enum(Rc::new(EnumVal {
            enum_name: intern("Pick"),
            variant: intern("Ghost"),
            fields: Fields::from_pairs([]),
        }));
        assert!(
            cast(ix, &ghost, "Pick").is_err(),
            "a variant the enum lacks"
        );
        let other = Value::Enum(Rc::new(EnumVal {
            enum_name: intern("Other"),
            variant: intern("One"),
            fields: Fields::from_pairs([(intern("x"), Value::Int(1))]),
        }));
        assert!(
            cast(ix, &other, "Pick").is_err(),
            "ATTACK: another enum's look-alike variant passed by variant name"
        );
        // A refinement by its base.
        assert!(cast(ix, &Value::Int(1), "Pos").is_ok());
        assert!(cast(ix, &strv("s"), "Pos").is_err());
    });
}

/// Containers element by element; a union by its first conforming member.
#[test]
fn containers_are_cast_element_by_element() {
    with_ix("r51-s3-cont", false, |ix| {
        let some = Value::Some(Box::new(Value::Int(1)));
        assert!(cast(ix, &some, "Option<i64>").is_ok());
        assert!(cast(ix, &Value::None, "Option<i64>").is_ok());
        assert!(cast(ix, &some, "Option<str>").is_err());
        assert!(cast(ix, &Value::Int(1), "Option<i64>").is_err());
        assert!(cast(ix, &Value::Ok(Box::new(Value::Int(1))), "Result<i64, str>").is_ok());
        assert!(cast(ix, &Value::Err(Box::new(Value::Int(1))), "Result<i64, str>").is_err());
        let arr = Value::Array(Rc::new(vec![Value::Int(1), strv("s")]));
        assert!(cast(ix, &arr, "[i64]").is_err(), "ATTACK: a stray element");
        let arr = Value::Array(Rc::new(vec![Value::Int(1), Value::Int(2)]));
        let out = cast(ix, &arr, "[u8]").expect("converted in place");
        let Value::Array(xs) = &out else {
            unreachable!()
        };
        assert!(matches!(
            xs[1],
            Value::SizedInt {
                val: 2,
                ty: IntWidth::U8
            }
        ));
        let tup = Value::tuple(vec![Value::Int(1), strv("s")]);
        assert!(cast(ix, &tup, "(i64, str)").is_ok());
        assert!(cast(ix, &tup, "(str, i64)").is_err());
        assert!(cast(ix, &tup, "(i64, str, i64)").is_err());
        assert!(cast(ix, &strv("s"), "i64 | str").is_ok());
        assert!(cast(ix, &Value::Bool(true), "i64 | str").is_err());
    });
}

/// Soft typing: a wrapper stands for its plain `T`; where the declared type
/// fixes the runtime type it is REPLACED by its inner value, so a method call
/// dispatches on the declared type rather than on the wrapper.
#[test]
fn a_soft_wrapper_is_replaced_where_the_declared_type_fixes_the_runtime_type() {
    with_ix("r51-s3-soft", false, |ix| {
        let u = soft("Uncertain", Value::Int(5));
        assert!(matches!(cast(ix, &u, "i64"), Ok(Value::Int(5))));
        assert!(matches!(
            cast(ix, &u, "u8"),
            Ok(Value::SizedInt {
                val: 5,
                ty: IntWidth::U8
            })
        ));
        assert!(
            cast(ix, &u, "str").is_err(),
            "the inner value is still cast"
        );
        assert!(
            matches!(cast(ix, &u, "?"), Ok(Value::Struct(_))),
            "kept where unstated"
        );
        assert!(matches!(
            cast(ix, &u, "Uncertain<i64>"),
            Ok(Value::Struct(_))
        ));
        assert!(cast(ix, &u, "Uncertain<str>").is_err());
        assert!(cast(ix, &u, "Temporal<i64>").is_err(), "the other wrapper");
        assert!(
            cast(ix, &Value::Int(5), "Uncertain<i64>").is_ok(),
            "a plain T"
        );
        assert!(cast(ix, &Value::Int(5), "Uncertain").is_err());
        // A name the interpreter does not know keeps the wrapper.
        assert!(matches!(cast(ix, &u, "NoSuchType"), Ok(Value::Struct(_))));
        // A type parameter binds from the inner value (veto conform.rs:425-431:
        // the inner cast binds it, and the binding then pins the type), so the
        // wrapper is replaced, and a later value at `T` must be that type.
        let cx = cx_of(ix, "same").fresh();
        let mut v = u.clone();
        ix.cast(&mut v, &ty("T"), &cx).expect("binds T");
        assert!(matches!(v, Value::Int(5)));
        assert!(ix.cast(&mut strv("s"), &ty("T"), &cx).is_err());
    });
}

/// `dyn Trait` and a type parameter's bounds need an impl of the trait the
/// program defines.
#[test]
fn a_trait_object_and_a_bound_need_an_impl() {
    with_ix("r51-s3-dyn", false, |ix| {
        assert!(cast(ix, &run(ix, "sq"), "dyn Shape").is_ok());
        assert!(
            cast(ix, &run(ix, "fake"), "dyn Shape").is_err(),
            "ATTACK: a type without the impl passed as `dyn Shape`"
        );
        let cx = cx_of(ix, "area_of").fresh();
        let mut v = run(ix, "fake");
        assert!(
            ix.cast(&mut v, &ty("T"), &cx).is_err(),
            "the bound `T: Shape`"
        );
        let mut v = run(ix, "sq");
        assert!(ix.cast(&mut v, &ty("T"), &cx).is_ok());
    });
}

/// A type parameter is BOUND by the first value that meets it and every
/// later value agrees; a position nothing determined fills from a later
/// operator value; at a strict (seal) crossing an unbound one is refused.
#[test]
fn a_type_parameter_binds_once_and_a_strict_crossing_refuses_it_unbound() {
    with_ix("r51-s3-tp", false, |ix| {
        let cx = cx_of(ix, "same").fresh();
        let t = ty("T");
        assert!(ix.cast(&mut Value::Int(1), &t, &cx).is_ok());
        assert!(
            ix.cast(&mut strv("s"), &t, &cx).is_err(),
            "ATTACK: a second type at the same parameter"
        );
        // `fresh` is one activation: the next starts unbound.
        assert!(ix.cast(&mut strv("s"), &t, &cx.fresh()).is_ok());
        // `[]` leaves the element undetermined; `[1]` fills it; `["s"]` is refused.
        let cx = cx_of(ix, "same").fresh();
        let empty = Value::Array(Rc::new(Vec::new()));
        assert!(ix.cast(&mut empty.clone(), &t, &cx).is_ok());
        assert!(ix
            .cast(&mut Value::Array(Rc::new(vec![Value::Int(1)])), &t, &cx)
            .is_ok());
        assert!(
            ix.cast(&mut Value::Array(Rc::new(vec![strv("s")])), &t, &cx)
                .is_err(),
            "ATTACK: the filled element type was not kept"
        );
        // Strict and unbound: no honest body produces one.
        let strict = cx_of(ix, "same").fresh().strict(true);
        let e = ix
            .cast(&mut Value::Int(1), &t, &strict)
            .expect_err("refused");
        assert!(e.contains("no argument determined"), "{e}");
    });
    // `Self` reads the impl's type.
    let (prog, _) = prog("r51-s3-self");
    let ix = Interp::build_with(&prog, &[], Engine::Tree);
    let imp = prog
        .items
        .iter()
        .find_map(|it| match it {
            Item::ImplBlock(b) if b.trait_name == "Same" => Some(b),
            _ => None,
        })
        .expect("impl Same for Sq");
    let me = imp.methods.iter().find(|m| m.name == "me").expect("me");
    let cx = Cx::of_fn(me, Some(imp));
    assert!(ix.cast(&mut run(&ix, "sq"), &ty("Self"), &cx).is_ok());
    assert!(
        ix.cast(&mut run(&ix, "fake"), &ty("Self"), &cx).is_err(),
        "ATTACK: a look-alike passed as `Self`"
    );
}

/// A generic record's field environment: each type argument binds its
/// parameter; a MISSING argument is undetermined (refused when strict).
#[test]
fn a_generic_records_fields_are_read_at_its_type_arguments() {
    with_ix("r51-s3-gen", false, |ix| {
        let w = run(ix, "wrap_i");
        assert!(cast(ix, &w, "Wrap<i64>").is_ok());
        assert!(
            cast(ix, &run(ix, "wrap_s"), "Wrap<i64>").is_err(),
            "ATTACK: a field at another type argument"
        );
        assert!(cast(ix, &w, "Wrap").is_ok(), "lenient: undetermined passes");
        let mut v = w.clone();
        assert!(
            ix.cast(&mut v, &ty("Wrap"), &Cx::default().strict(true))
                .is_err(),
            "ATTACK: a strict crossing passed a field nothing determined"
        );
        assert_eq!(
            rt(&ix.value_type(&w, 0)),
            "Wrap<i64>",
            "value_type reads the argument"
        );
        let none_t = crate::doc::render_type(&ix.value_type(&Value::None, 0));
        assert_eq!(none_t, "Option<?undetermined>");
    });
}

/// Only a closure is a fn value; its arity must match.
#[test]
fn a_fn_type_takes_only_a_closure_of_that_arity() {
    with_ix("r51-s3-fnty", false, |ix| {
        assert!(cast(ix, &Value::Int(1), "fn(i64) -> i64").is_err());
        let e = cast(ix, &id_closure(), "fn(i64, i64) -> i64").expect_err("arity");
        assert!(e.contains("1 parameter"), "{e}");
    });
}

/// The contract cast on every call: arguments outermost first, the result
/// innermost first, and at a seal crossing every argument must meet a
/// position some contract determined.
#[test]
fn a_closure_contract_casts_its_arguments_and_result() {
    with_ix("r51-s3-cc", false, |ix| {
        let c = cast(ix, &id_closure(), "fn(i64) -> i64").expect("conforms");
        let k = &closure_of(&c).contract;
        let mut ok = [Value::Int(1)];
        assert!(ix.closure_args_check(k, &mut ok, false).is_ok());
        let mut bad = [strv("s")];
        assert!(
            refused(&ix.closure_args_check(k, &mut bad, false), "argument 1"),
            "ATTACK: an argument of another type reached the body"
        );
        let mut r = Value::Int(1);
        assert!(ix.closure_ret_check(k, &mut r, false).is_ok());
        let mut r = strv("s");
        assert!(
            refused(&ix.closure_ret_check(k, &mut r, false), "returned"),
            "ATTACK: a result of another type left the closure"
        );
        // A crossing needs a determined position: `?` is not one.
        let open = cast(ix, &id_closure(), "fn(?) -> ?").expect("conforms");
        let mut a = [Value::Int(1)];
        assert!(
            refused(
                &ix.closure_args_check(&closure_of(&open).contract, &mut a, true),
                "nothing on the operator side determined"
            ),
            "ATTACK: sealed code chose an argument type no operator value fixed"
        );
        assert!(ix.closure_args_check(k, &mut [Value::Int(1)], true).is_ok());
        assert!(ix
            .closure_args_check(&None, &mut [Value::Int(1)], false)
            .is_ok());
    });
}

/// A lambda's own annotations are its first contract; an unannotated lambda
/// has none.
#[test]
fn a_lambdas_annotations_are_its_first_contract() {
    use crate::ast::LambdaParam;
    assert!(Interp::lambda_contract(&[LambdaParam::untyped("x")]).is_none());
    let c = Interp::lambda_contract(&[
        LambdaParam::untyped("x"),
        LambdaParam {
            name: "y".into(),
            ty: Some(ty("str")),
        },
    ]);
    assert_eq!(rts(&Interp::param_types(&c, 0)), ["?"]);
    assert_eq!(rts(&Interp::param_types(&c, 1)), ["str"]);
    assert!(Interp::param_types(&c, 2).is_empty());
}

/// A record or variant field given at construction is cast to its declared
/// type.
#[test]
fn a_field_is_cast_to_its_declared_type() {
    with_ix("r51-s3-field", false, |ix| {
        assert!(ix.cast_field("Sq", "s", &mut Value::Int(1)).is_ok());
        assert!(ix.cast_field("Sq", "s", &mut strv("s")).is_err());
        assert!(ix.cast_field("Pick::One", "x", &mut strv("s")).is_err());
        assert!(ix.cast_field("Pick::One", "x", &mut Value::Int(1)).is_ok());
        assert!(
            ix.cast_field("Wrap", "v", &mut strv("s")).is_ok(),
            "its own T is free"
        );
        assert!(ix.cast_field("Nope", "s", &mut strv("s")).is_ok());
        assert!(ix.cast_field("Pick::Ghost", "x", &mut strv("s")).is_ok());
    });
}

// -- channels ----------------------------------------------------------------------

/// A channel is one invariant object: its creation type stamps it, a generic
/// `Chan<T>` binds `T` from the stamp, queued values are cast at a crossing,
/// and every send is cast.
#[test]
fn a_channel_is_stamped_once_and_every_send_is_cast() {
    with_ix("r51-s3-chan", false, |ix| {
        let q = chan();
        ix.chan_created(&q, Some("i64"));
        assert_eq!(rt(&ix.value_type(&Value::Chan(q.clone()), 0)), "Chan<i64>");
        assert!(ix.chan_send_check(&q, &mut Value::Int(1)).is_ok());
        assert!(
            refused(
                &ix.chan_send_check(&q, &mut strv("s")),
                "runtime type confusion"
            ),
            "ATTACK: a send of another type on a stamped channel"
        );
        // `Chan<T>` binds T from the stamp; a later T must agree.
        let cx = cx_of(ix, "take").fresh();
        let mut c = Value::Chan(q.clone());
        assert!(ix.cast(&mut c, &ty("Chan<T>"), &cx).is_ok());
        assert!(ix.cast(&mut strv("s"), &ty("T"), &cx).is_err(), "T is i64");
        // A type that is not closed does not stamp.
        let open = chan();
        ix.chan_created(&open, Some("Wrap<T>"));
        assert_eq!(
            crate::doc::render_type(&ix.value_type(&Value::Chan(open.clone()), 0)),
            "Chan<?undetermined>"
        );
        // Queued values are cast when the channel crosses a type.
        let full = chan();
        full.borrow_mut().push_back(Value::Int(1));
        let mut c = Value::Chan(full.clone());
        assert!(ix.cast(&mut c, &ty("Chan<str>"), &Cx::default()).is_err());
        assert!(ix.cast(&mut c, &ty("Chan<i64>"), &Cx::default()).is_ok());
        assert!(
            ix.chan_send_check(&full, &mut strv("s")).is_err(),
            "stamped by the crossing"
        );
        // Strict and unbound: refused.
        let mut c = Value::Chan(chan());
        let e = ix
            .cast(
                &mut c,
                &ty("Chan<T>"),
                &cx_of(ix, "take").fresh().strict(true),
            )
            .expect_err("refused");
        assert!(e.contains("no argument determined"), "{e}");
        assert!(ix
            .cast(&mut Value::Int(1), &ty("Chan<i64>"), &Cx::default())
            .is_err());
    });
}

/// A SEALED frame's send on a channel the OPERATOR created needs an element
/// type some operator-side value determined.
#[test]
fn a_sealed_send_on_an_operator_channel_needs_a_determined_element_type() {
    with_ix("r51-s3-chan-s", true, |ix| {
        let q = chan();
        ix.chan_created(&q, None);
        assert!(
            ix.chan_send_check(&q, &mut Value::Int(1)).is_ok(),
            "the operator's own send"
        );
        let r = ix.with_frame(true, || ix.chan_send_check(&q, &mut Value::Int(1)));
        assert!(
            refused(&r, "nothing on the operator side determined"),
            "ATTACK: the candidate chose the element type of the operator's channel"
        );
        let typed = chan();
        ix.chan_created(&typed, Some("i64"));
        let r = ix.with_frame(true, || ix.chan_send_check(&typed, &mut Value::Int(1)));
        assert!(r.is_ok());
        // A channel the CANDIDATE created is its own.
        let own = chan();
        ix.with_frame(true, || ix.chan_created(&own, None));
        let r = ix.with_frame(true, || ix.chan_send_check(&own, &mut Value::Int(1)));
        assert!(r.is_ok());
    });
}

// -- the operator's dicts ----------------------------------------------------------

/// A dict the operator hands sealed code is snapshotted; a value sealed code
/// stores at a key the operator held must keep its type; keys it adds are
/// free.
#[test]
fn a_dict_entry_the_operator_held_is_never_retyped() {
    with_ix("r51-s3-dict", true, |ix| {
        let d = dict(&[("k", Value::Int(1))]);
        ix.dict_edge_in(&Value::Array(Rc::new(vec![Value::Dict(d.clone())])))
            .expect("in");
        ix.with_frame(true, || {
            ix.dict_mutated(&d);
            d.borrow_mut().insert("new".into(), strv("free"));
            d.borrow_mut().insert("k".into(), Value::Int(2));
        });
        assert!(
            ix.dict_edge_out().is_ok(),
            "same type at a held key, a new key"
        );
        ix.with_frame(true, || {
            ix.dict_mutated(&d);
            d.borrow_mut().insert("k".into(), strv("s"));
        });
        assert!(
            refused(&ix.dict_edge_out(), "key `k`"),
            "ATTACK: sealed code retyped a dict entry the operator held"
        );
    });
}

/// The operator mutating a dict after the hand-over makes the snapshot stale:
/// the next hand-over re-takes it, and the FIRST sealed mutation records what
/// the dict then holds.
#[test]
fn an_operator_mutation_refreshes_the_snapshot() {
    with_ix("r51-s3-dict-epoch", true, |ix| {
        let d = dict(&[("k", Value::Int(1))]);
        ix.dict_edge_in(&Value::Dict(d.clone())).expect("in");
        d.borrow_mut().insert("k".into(), strv("now a str"));
        ix.dict_mutated(&d);
        ix.dict_edge_in(&Value::Dict(d.clone()))
            .expect("re-snapshot");
        ix.with_frame(true, || {
            ix.dict_mutated(&d);
            d.borrow_mut().insert("k".into(), strv("still a str"));
        });
        assert!(
            ix.dict_edge_out().is_ok(),
            "judged against the operator's latest"
        );
        // Unsealed: no snapshot is taken at all.
    });
    with_ix("r51-s3-dict-u", false, |ix| {
        let d = dict(&[("k", Value::Int(1))]);
        ix.dict_edge_in(&Value::Dict(d.clone())).expect("in");
        assert!(ix.dict_snaps.borrow().is_empty());
    });
}

/// A dict reached only inside another dict, a record, an enum payload or a
/// channel queue is snapshotted too.
#[test]
fn a_nested_dict_is_snapshotted() {
    with_ix("r51-s3-dict-nest", true, |ix| {
        let inner = dict(&[("x", Value::Int(1))]);
        let q = chan();
        q.borrow_mut().push_back(Value::Dict(inner.clone()));
        let v = record("Sq", &[("s", Value::Some(Box::new(Value::Chan(q))))]);
        ix.dict_edge_in(&v).expect("in");
        assert_eq!(ix.dict_snaps.borrow().len(), 1);
        ix.with_frame(true, || {
            ix.dict_mutated(&inner);
            inner.borrow_mut().insert("x".into(), Value::Bool(true));
        });
        assert!(refused(&ix.dict_edge_out(), "key `x`"));
    });
}

/// A dict reached only as another dict's value is snapshotted too: the walk
/// goes into a dict's values.
#[test]
fn a_dict_inside_a_dict_is_snapshotted() {
    with_ix("r51-s3-dict-in-dict", true, |ix| {
        let inner = dict(&[("x", Value::Int(1))]);
        let outer = dict(&[("in", Value::Dict(inner.clone()))]);
        ix.dict_edge_in(&Value::Dict(outer)).expect("in");
        assert_eq!(ix.dict_snaps.borrow().len(), 2, "both dicts");
        ix.with_frame(true, || {
            ix.dict_mutated(&inner);
            inner.borrow_mut().insert("x".into(), strv("s"));
        });
        assert!(
            refused(&ix.dict_edge_out(), "key `x`"),
            "ATTACK: a dict inside a handed dict was retyped unrecorded"
        );
    });
}

/// A dict too large to snapshot is refused at the crossing, never skipped.
#[test]
fn a_dict_past_the_snapshot_bound_is_refused_not_skipped() {
    with_ix("r51-s3-dict-max", true, |ix| {
        let d: std::collections::BTreeMap<String, Value> = (0..=super::conform::DICT_SNAP_MAX)
            .map(|i| (i.to_string(), Value::Unit))
            .collect();
        let r = ix.dict_edge_in(&Value::Dict(Rc::new(RefCell::new(d))));
        assert!(
            refused(&r, "cannot cross a seal"),
            "ATTACK: a dict past the bound crossed with no snapshot: {r:?}"
        );
    });
}

/// An operator mutation (`dict_mutated` outside a sealed frame bumps
/// `dict_epoch`) makes every snapshot stale, so the next hand-over re-takes
/// it: a replacing dict is then judged against what the operator holds NOW.
#[test]
fn a_handover_after_an_operator_mutation_retakes_the_snapshot() {
    with_ix("r51-s3-dict-retake", true, |ix| {
        let d = dict(&[("k", Value::Int(1))]);
        ix.dict_edge_in(&Value::Dict(d.clone())).expect("in");
        d.borrow_mut().insert("k".into(), strv("now a str"));
        ix.dict_mutated(&d);
        ix.dict_edge_in(&Value::Dict(d.clone())).expect("again");
        let now = Value::Dict(dict(&[("k", strv("s"))]));
        assert!(
            ix.replaced_ok_top(&Value::Dict(d.clone()), &now).is_ok(),
            "judged against the operator's latest"
        );
        let first = Value::Dict(dict(&[("k", Value::Int(2))]));
        assert!(
            ix.replaced_ok_top(&Value::Dict(d), &first).is_err(),
            "ATTACK: judged against a stale snapshot"
        );
    });
}

/// A snapshot taken since the last operator mutation, and not dirtied, is
/// KEPT at the next hand-over (O(1) per dict), not re-taken from whatever
/// the dict holds by then.
#[test]
fn a_fresh_snapshot_is_kept_at_the_next_handover() {
    with_ix("r51-s3-dict-fresh", true, |ix| {
        let d = dict(&[("k", Value::Int(1))]);
        ix.dict_edge_in(&Value::Dict(d.clone())).expect("in");
        // A write no mutation hook saw: a re-take would record it.
        d.borrow_mut().insert("k".into(), strv("unseen"));
        ix.dict_edge_in(&Value::Dict(d.clone())).expect("again");
        let replacing = Value::Dict(dict(&[("k", Value::Int(2))]));
        assert!(
            ix.replaced_ok_top(&Value::Dict(d), &replacing).is_ok(),
            "the kept snapshot holds `k: i64`"
        );
    });
}

/// Round 6: a candidate closure that captured a handed dict mutates it after
/// the OPERATOR did, with no hand-over in between. The first sealed mutation
/// records what the dict then holds, so the operator's own change is what
/// sealed code is judged against.
#[test]
fn the_first_sealed_mutation_records_what_the_operator_then_held() {
    with_ix("r51-s3-dict-round6", true, |ix| {
        let d = dict(&[("k", Value::Int(1))]);
        ix.dict_edge_in(&Value::Dict(d.clone())).expect("in");
        d.borrow_mut().insert("k".into(), strv("op"));
        ix.dict_mutated(&d);
        ix.with_frame(true, || {
            ix.dict_mutated(&d);
            d.borrow_mut().insert("k".into(), strv("cand"));
        });
        assert!(
            ix.dict_edge_out().is_ok(),
            "an honest write of the operator's latest type"
        );
        ix.with_frame(true, || {
            ix.dict_mutated(&d);
            d.borrow_mut().insert("k".into(), Value::Int(3));
        });
        assert!(
            refused(&ix.dict_edge_out(), "key `k`"),
            "ATTACK: retyped against the operator's latest"
        );
    });
}

/// What replaces a held value is judged by what the operator held, deeply
/// (amendment 78).
#[test]
fn a_replacement_is_judged_by_what_the_operator_held() {
    with_ix("r51-s3-repl", true, |ix| {
        assert!(ix.replaced_ok_top(&Value::Int(1), &Value::Int(2)).is_ok());
        assert!(ix.replaced_ok_top(&Value::Int(1), &strv("s")).is_err());
        // Not strict at the top: an honest callee fills an empty array.
        let empty = Value::Array(Rc::new(Vec::new()));
        assert!(ix
            .replaced_ok_top(&empty, &Value::Array(Rc::new(vec![Value::Int(1)])))
            .is_ok());
        // An operator closure is never replaced by the candidate's.
        let op = id_closure();
        let cand = ix.with_frame(true, || run(ix, "adder"));
        assert!(ix.replaced_ok_top(&op, &op.clone()).is_ok());
        assert!(
            ix.replaced_ok_top(&op, &cand).is_err(),
            "ATTACK: the candidate's closure replaced the operator's"
        );
        assert!(ix.replaced_ok_top(&op, &Value::Int(1)).is_err());
        // A closure's signature is not shown, so it is not judged.
        let arr = |c: &Value| Value::Array(Rc::new(vec![c.clone()]));
        assert!(ix.replaced_ok_top(&arr(&op), &arr(&id_closure())).is_ok());
        // A replacing dict is judged against the held one's entries, inside
        // a container.
        let held = Value::Some(Box::new(Value::Dict(dict(&[("a", Value::Int(1))]))));
        let good = Value::Some(Box::new(Value::Dict(dict(&[
            ("a", Value::Int(2)),
            ("b", strv("new")),
        ]))));
        let bad = Value::Some(Box::new(Value::Dict(dict(&[("a", strv("s"))]))));
        assert!(ix.replaced_ok_top(&held, &good).is_ok());
        assert!(
            ix.replaced_ok_top(&held, &bad).is_err(),
            "ATTACK: a replacing dict retyped a held key"
        );
        // Strict inside a held dict: `None` did not show its payload.
        let d = dict(&[("k", Value::None)]);
        ix.dict_edge_in(&Value::Dict(d.clone())).expect("in");
        ix.with_frame(true, || {
            ix.dict_mutated(&d);
            d.borrow_mut()
                .insert("k".into(), Value::Some(Box::new(Value::Int(1))));
        });
        assert!(
            refused(&ix.dict_edge_out(), "key `k`"),
            "ATTACK: a position the operator's value did not show was filled freely"
        );
    });
}

/// Inside an array the operator held, each element is judged by what the
/// operator held at that index, deeply (a dict element keeps its keys' types
/// although the array's own type says nothing of them).
#[test]
fn an_element_of_a_held_array_is_judged_deeply() {
    with_ix("r51-s3-repl-arr", true, |ix| {
        let arr = |m| Value::Array(Rc::new(vec![Value::Dict(m)]));
        let held = arr(dict(&[("a", Value::Int(1))]));
        assert!(ix
            .replaced_ok_top(&held, &arr(dict(&[("a", Value::Int(2))])))
            .is_ok());
        assert!(
            ix.replaced_ok_top(&held, &arr(dict(&[("a", strv("s"))])))
                .is_err(),
            "ATTACK: an array element's dict entry was retyped"
        );
    });
}

/// The effect-handler edges: an operator arm for a sealed performer is a
/// return to operator code; a sealed arm for an operator performer is a
/// hand-over; the arm's resume goes back across the same edge.
#[test]
fn a_handler_arm_is_an_edge_between_its_provenance_and_the_performers() {
    with_ix("r51-s3-handler", true, |ix| {
        let d = dict(&[("k", Value::Int(1))]);
        // Operator performs, sealed arm: the payload is handed over.
        ix.handler_edge_into(true, &Value::Dict(d.clone()))
            .expect("in");
        assert_eq!(ix.dict_snaps.borrow().len(), 1);
        // Sealed performer, operator arm: the dicts are checked on the way out.
        ix.with_frame(true, || {
            ix.dict_mutated(&d);
            d.borrow_mut().insert("k".into(), strv("s"));
        });
        let r = ix.with_frame(true, || ix.handler_edge_into(false, &Value::Unit));
        assert!(
            refused(&r, "key `k`"),
            "ATTACK: the arm saw a retyped entry"
        );
        // The operator arm resumes the sealed performer with a dict: handed in.
        let e = dict(&[("j", Value::Int(1))]);
        ix.with_frame(true, || {
            ix.handler_edge_back(false, &Value::Dict(e.clone()))
        })
        .expect("back");
        assert!(
            ix.dict_snaps.borrow().len() >= 2,
            "the resume value was snapshotted"
        );
        // Same provenance: no edge.
        assert!(ix.handler_edge_back(false, &Value::Unit).is_ok());
    });
}

// -- channel access (amendments 106, 117; veto eval.rs:650-658, 1019-1034) ---------

/// Run the suite fn `name` on `args` (value, taint) in the sealed
/// instantiation, in a sealed frame or an operator one; its value and taint.
fn probe(ix: &Interp<'_>, name: &str, sealed: bool, args: Vec<(Value, u8)>) -> (Value, u8) {
    let f = *ix.fns.get(name).expect("a probe of the suite");
    let t = &ix.taint;
    for c in [&t.acc, &t.last, &t.pc, &t.sticky, &t.ret] {
        c.set(0);
    }
    let mut env = ix.new_env();
    let taints: Vec<u8> = args.iter().map(|(_, t)| *t).collect();
    env.bind_fresh(
        f.params
            .iter()
            .zip(args)
            .map(|(p, (v, _))| (intern(&p.name), v)),
        |i| taints[i],
    );
    match ix.with_frame(sealed, || ix.eval_t::<true>(&f.body, &mut env)) {
        Ok(v) => (v, ix.taint.last.get()),
        Err(Flow::Return(v)) => (v, ix.taint.ret.get()),
        Err(e) => panic!("{name}: {e:?}"),
    }
}

/// A channel holding one value.
fn full_chan() -> Rc<RefCell<std::collections::VecDeque<Value>>> {
    let q = chan();
    q.borrow_mut().push_back(Value::Int(7));
    q
}

/// An OPERATOR method call on a channel under the candidate's control, or on
/// a channel the candidate's value chose, is a write the candidate decided:
/// the channel is marked (`t_chan_choice`), so whatever reads it later is
/// tainted. The same call under the operator's own control marks nothing.
#[test]
fn an_operator_channel_access_the_candidate_controls_marks_the_channel() {
    with_ix("r51-s3-chan-choice", true, |ix| {
        for (c, want) in [(taint::ALL, taint::VAL), (0, 0)] {
            let q = full_chan();
            probe(
                ix,
                "ch_len",
                false,
                vec![(Value::Int(1), c), (Value::Chan(q.clone()), 0)],
            );
            assert_eq!(
                ix.t_obj(&Value::Chan(q)) & taint::VAL,
                want,
                "ATTACK: `len` under a branch on a value of taint {c}"
            );
            let (q, r) = (chan(), chan());
            probe(
                ix,
                "ch_pick",
                false,
                vec![
                    (Value::Int(1), c),
                    (Value::Chan(q.clone()), 0),
                    (Value::Chan(r), 0),
                ],
            );
            assert_eq!(
                ix.t_obj(&Value::Chan(q)) & taint::VAL,
                want,
                "ATTACK: `len` of a channel a value of taint {c} chose"
            );
        }
    });
}

/// A SEALED drain (`recv`) changes what the operator's queue holds: the
/// channel is marked ALL (`t_chan_access`). A sealed `len` changes nothing and
/// marks nothing; an operator read of a marked channel carries its taint.
#[test]
fn a_sealed_drain_marks_the_channel_and_a_read_carries_the_mark() {
    with_ix("r51-s3-chan-access", true, |ix| {
        let q = full_chan();
        probe(
            ix,
            "ch_recv",
            true,
            vec![(Value::Int(0), 0), (Value::Chan(q.clone()), 0)],
        );
        assert_eq!(
            ix.t_obj(&Value::Chan(q.clone())),
            taint::ALL,
            "ATTACK: a sealed drain left the operator's channel clean"
        );
        let quiet = full_chan();
        probe(
            ix,
            "ch_count",
            true,
            vec![(Value::Int(0), 0), (Value::Chan(quiet.clone()), 0)],
        );
        assert_eq!(
            ix.t_obj(&Value::Chan(quiet.clone())),
            0,
            "a sealed len marks nothing"
        );
        let (_, t) = probe(
            ix,
            "ch_count",
            false,
            vec![(Value::Int(0), 0), (Value::Chan(q), 0)],
        );
        assert_eq!(
            t & taint::ALL,
            taint::ALL,
            "ATTACK: len of a drained channel read clean"
        );
        let (_, t) = probe(
            ix,
            "ch_count",
            false,
            vec![(Value::Int(0), 0), (Value::Chan(quiet), 0)],
        );
        assert_eq!(t, 0, "len of an untouched channel");
    });
}

/// `select` pops a queue itself, so it takes and marks the channel the same
/// way a method call does: a sealed select drains (ALL), an operator select
/// under the candidate's control marks it.
#[test]
fn a_select_takes_and_marks_its_channel_like_a_recv() {
    with_ix("r51-s3-chan-select", true, |ix| {
        let q = full_chan();
        probe(
            ix,
            "ch_select",
            true,
            vec![(Value::Int(1), 0), (Value::Chan(q.clone()), 0)],
        );
        assert_eq!(
            ix.t_obj(&Value::Chan(q)),
            taint::ALL,
            "ATTACK: a sealed select drained a clean channel"
        );
        for (c, want) in [(taint::ALL, taint::VAL), (0, 0)] {
            let q = full_chan();
            probe(
                ix,
                "ch_select",
                false,
                vec![(Value::Int(1), c), (Value::Chan(q.clone()), 0)],
            );
            assert_eq!(
                ix.t_obj(&Value::Chan(q)) & taint::VAL,
                want,
                "ATTACK: an operator select under a branch on a value of taint {c}"
            );
        }
    });
}

/// The cast's own depth bound refuses, never waves the rest through.
#[test]
fn a_cast_past_its_depth_bound_is_refused() {
    with_ix("r51-s3-cast-depth", false, |ix| {
        let t = ty("i64");
        assert!(ix
            .cast_at(&mut Value::Int(1), &t, &Cx::default(), MAX_CAST_DEPTH)
            .is_ok());
        let past = ix.cast_at(&mut Value::Int(1), &t, &Cx::default(), MAX_CAST_DEPTH + 1);
        assert!(
            past.as_ref()
                .is_err_and(|e| e.contains("nested too deeply")),
            "ATTACK: a value past the cast bound was not refused: {past:?}"
        );
    });
}
