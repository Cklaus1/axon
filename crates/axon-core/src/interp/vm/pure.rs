//! R50 S7: pure scalar regions (spec §4 S7). A *pure tree* is an expression
//! built only from identifiers that resolution binds to a local slot, `Int`,
//! `Float` and `Bool` literals, and binary operators (`&&`/`||` included)
//! over pure trees, so evaluating it has no effect besides its value or its
//! panic. Three users run pure trees without the operand stack:
//!
//! - [`Op::Pure`]: the value of a sink (an `x = ..` that is not AX-31-shaped,
//!   an untyped `let`, an `if`/`while` condition), placed before the
//!   expression's generic ops (its twin), which run instead whenever it
//!   declines. The reads are pure, so running them twice is unobservable.
//! - [`Op::PureLoop`] / [`Op::PureFor`]: a `while` or a `for` over an
//!   integer range whose body is pure statements (assignments to scalar
//!   locals, element writes to local arrays, `if`s over them; spec §4 S10),
//!   run in registers, placed before the generic loop, which takes over on
//!   a decline. Inside a loop `xs[i]` on a local array is a leaf too.
//! - [`Interp::fold_leaf`]: `arr_fold` over a closure whose body is one pure
//!   tree.
//!
//! Every path declines (`None`, `false`, `0`) wherever the spec's rules say
//! so: a leaf that is unbound or holds anything but a plain `Int`, `Float`
//! or `Bool`; operands of different kinds; an int or float operation with no
//! [`int_fast`]/[`float_fast`] arm (overflow, a zero divisor, ...); `&&`/`||`
//! on a non-`Bool`; an operator other than `&&`, `||`, `==` and `!=` on two
//! `Bool`s. [`pure_eval`] is the reference for those rules. The register
//! code ([`Typed`], [`LoopCode`]) is specialised on the leaf kinds of its
//! first run and re-checks them on every later run, declining on a mismatch.

use super::*;
use std::cell::OnceCell;

/// A pure tree (module docs). `Local` reads with [`Env::get_var`].
pub(super) enum Pure<'p> {
    Local(Var<'p>),
    Int(i64),
    Float(f64),
    Bool(bool),
    /// Any operator but `&&`/`||`.
    Bin(BinOp, Box<[Pure<'p>; 2]>),
    And(Box<[Pure<'p>; 2]>),
    Or(Box<[Pure<'p>; 2]>),
    /// `xs[i]` on a local `xs` (loops only, spec §4 S10).
    Index(Var<'p>, Box<Pure<'p>>),
}

/// Where an [`Op::Pure`] delivers its value.
pub(super) enum Sink<'p> {
    /// `x = value` (not AX-31-shaped), as [`Op::Store`] stores it.
    Store(Var<'p>),
    /// An untyped `let`, as [`Op::Define`] binds it.
    Define(Var<'p>),
    /// An `if`/`while` condition: jump to `target` when it is `false`; when
    /// `true` and `push` (a `while`), `env.push()` for the iteration, as the
    /// twin's branch op does. Any other value declines, so the twin's
    /// `cond_bool` panics.
    Branch { target: u32, push: bool },
}

/// An [`Op::Pure`]: the tree, its sink, and the op after its twin.
pub(in crate::interp) struct PureOp<'p> {
    pub(super) e: PureExpr<'p>,
    pub(super) sink: Sink<'p>,
    /// The op after the twin (the generic ops of the same expression and
    /// sink), where a delivered value continues.
    pub(super) skip: u32,
}

impl PureOp<'_> {
    /// Deliver the tree's value to the sink: the op to continue at, or
    /// `None` (declined: the twin runs next, nothing was written).
    #[inline(always)]
    pub(super) fn run(&self, env: &mut Env, scopes: &mut u32) -> Option<u32> {
        let s = self.e.eval(env)?;
        match &self.sink {
            // As `store` writes: an int over an int and a float over a
            // float in place. An unbound local declines, so the twin
            // panics as the tree does.
            Sink::Store(var) => match (env.get_var_mut(var.s, var.slot)?, s) {
                (Value::Int(d), Scalar::Int(n)) => *d = n,
                (Value::Float(d), Scalar::Float(f)) => *d = f,
                (b, s) => *b = s.value(),
            },
            Sink::Define(var) => env.define_var(var.s, var.slot, s.value()),
            Sink::Branch { target, push } => match s {
                Scalar::Bool(false) => return Some(*target),
                Scalar::Bool(true) => {
                    if *push {
                        env.push();
                        *scopes += 1;
                    }
                }
                _ => return None,
            },
        }
        Some(self.skip)
    }
}

/// The kind of a register's bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    I,
    F,
    B,
}

/// The kind of a plain `Int`, `Float` or `Bool`; `None` for every other
/// value (`SizedInt`, `Uncertain`, `Temporal`, `Str`, ...).
#[inline(always)]
fn kind_of(v: &Value) -> Option<Kind> {
    match v {
        Value::Int(_) => Some(Kind::I),
        Value::Float(_) => Some(Kind::F),
        Value::Bool(_) => Some(Kind::B),
        _ => None,
    }
}

/// `v`'s bits when it is a plain value of kind `k`.
#[inline(always)]
fn bits(v: &Value, k: Kind) -> Option<u64> {
    match (v, k) {
        (Value::Int(n), Kind::I) => Some(*n as u64),
        (Value::Float(f), Kind::F) => Some(f.to_bits()),
        (Value::Bool(b), Kind::B) => Some(*b as u64),
        _ => None,
    }
}

/// The value register bits `x` of kind `k` stand for.
#[inline(always)]
fn scalar(x: u64, k: Kind) -> Scalar {
    match k {
        Kind::I => Scalar::Int(x as i64),
        Kind::F => Scalar::Float(f64::from_bits(x)),
        Kind::B => Scalar::Bool(x != 0),
    }
}

/// One register instruction's operation. The dedicated arms compute exactly
/// what [`int_fast`]/[`float_fast`] compute for their operator (the unit test
/// `pure_ops_agree_with_scalar_fast` pins them); `OtherI`/`OtherF` call them.
#[derive(Clone, Debug)]
pub(super) enum TOp {
    AddI,
    SubI,
    MulI,
    LtI,
    LeI,
    GtI,
    GeI,
    /// `==` on two ints or two bools: their bits compare equal.
    EqI,
    NeI,
    OtherI(BinOp),
    AddF,
    SubF,
    MulF,
    DivF,
    LtF,
    LeF,
    GtF,
    GeF,
    EqF,
    NeF,
    OtherF(BinOp),
    /// `dst = a`; when `a` is false (`And`) or true (`Or`), skip the next
    /// `n` instructions (the right side).
    And(u32),
    Or(u32),
    /// `dst = a`.
    Mov,
    /// `dst = xs[a]`, array `.0` of the loop, elements of kind `.1`.
    Load(u8, Kind),
    /// `xs[a] = b` (no `dst`).
    Store(u8, Kind),
    /// When `a` is false skip the next `n` instructions (no `dst`).
    Br(u32),
    /// Skip the next `n` instructions (no operand, no `dst`).
    Jmp(u32),
}

/// A register operand before allocation: a local of a kind, a literal's
/// bits, or temporary `d`.
#[derive(Clone, Copy)]
enum Src {
    L(Sym, u32, Kind),
    K(u64),
    R(u8),
}

/// Three-address code before register allocation.
struct TIns {
    op: TOp,
    a: Src,
    b: Src,
    dst: u8,
}

/// One register instruction: `r[dst] = r[a] op r[b]`.
pub(super) struct RIns {
    op: TOp,
    a: u8,
    b: u8,
    dst: u8,
}

/// The most operators a pure tree nests (`Compiler::pure_tree`): a deeper
/// expression is compiled the generic way. AX-56: this bounds every
/// recursion over a pure tree (building, [`specialize`], [`pure_eval`],
/// `pure_locals`, dropping it), which the wasm32 stack budget does not
/// charge.
pub(super) const PURE_DEPTH: u32 = 16;

/// Leaf registers of a [`Typed`] (locals and literals); temporaries follow.
const LEAVES: u8 = 8;
/// Temporaries (expression depth) of one expression.
const TEMPS: u8 = 8;

/// The kinds [`specialize`] types leaves by: a local's, and a loop array's
/// index and element kind.
struct Kinds<'a> {
    local: &'a dyn Fn(Sym, u32) -> Option<Kind>,
    elem: &'a dyn Fn(Sym, u32) -> Option<(u8, Kind)>,
}

/// No arrays: the leaf kinds of an [`Op::Pure`] or a fold leaf.
fn no_elem(_: Sym, _: u32) -> Option<(u8, Kind)> {
    None
}

/// Three-address code for `p` into `code`, its value in temporary `d` or a
/// leaf operand, typed by `kinds`. `None` where a first run would decline
/// on these kinds (or the depth passes [`TEMPS`]): then no register code
/// exists and [`pure_eval`] runs instead.
fn specialize(p: &Pure<'_>, kinds: &Kinds<'_>, code: &mut Vec<TIns>, d: u8) -> Option<(Src, Kind)> {
    if d >= TEMPS {
        return None;
    }
    Some(match p {
        Pure::Local(v) => {
            let k = (kinds.local)(v.s, v.slot)?;
            (Src::L(v.s, v.slot, k), k)
        }
        Pure::Index(v, i) => {
            let (arr, k) = (kinds.elem)(v.s, v.slot)?;
            let (x, kx) = specialize(i, kinds, code, d)?;
            if kx != Kind::I {
                return None;
            }
            code.push(TIns {
                op: TOp::Load(arr, k),
                a: x,
                b: Src::K(0),
                dst: d,
            });
            (Src::R(d), k)
        }
        Pure::Int(n) => (Src::K(*n as u64), Kind::I),
        Pure::Float(f) => (Src::K(f.to_bits()), Kind::F),
        Pure::Bool(b) => (Src::K(*b as u64), Kind::B),
        Pure::Bin(op, k) => {
            let (a, ka) = specialize(&k[0], kinds, code, d)?;
            let (b, kb) = specialize(&k[1], kinds, code, d + 1)?;
            use BinOp::*;
            let (top, out) = match (ka, kb) {
                (Kind::I, Kind::I) => match op {
                    Add => (TOp::AddI, Kind::I),
                    Sub => (TOp::SubI, Kind::I),
                    Mul => (TOp::MulI, Kind::I),
                    Lt => (TOp::LtI, Kind::B),
                    LtEq => (TOp::LeI, Kind::B),
                    Gt => (TOp::GtI, Kind::B),
                    GtEq => (TOp::GeI, Kind::B),
                    Eq => (TOp::EqI, Kind::B),
                    NotEq => (TOp::NeI, Kind::B),
                    Div | Rem | BitAnd | BitOr | BitXor | Shl | Shr => {
                        (TOp::OtherI(op.clone()), Kind::I)
                    }
                    And | Or => return None,
                },
                (Kind::F, Kind::F) => match op {
                    Add => (TOp::AddF, Kind::F),
                    Sub => (TOp::SubF, Kind::F),
                    Mul => (TOp::MulF, Kind::F),
                    Div => (TOp::DivF, Kind::F),
                    Rem => (TOp::OtherF(Rem), Kind::F),
                    Lt => (TOp::LtF, Kind::B),
                    LtEq => (TOp::LeF, Kind::B),
                    Gt => (TOp::GtF, Kind::B),
                    GtEq => (TOp::GeF, Kind::B),
                    Eq => (TOp::EqF, Kind::B),
                    NotEq => (TOp::NeF, Kind::B),
                    And | Or | BitAnd | BitOr | BitXor | Shl | Shr => return None,
                },
                (Kind::B, Kind::B) => match op {
                    Eq => (TOp::EqI, Kind::B),
                    NotEq => (TOp::NeI, Kind::B),
                    _ => return None,
                },
                _ => return None,
            };
            code.push(TIns {
                op: top,
                a,
                b,
                dst: d,
            });
            (Src::R(d), out)
        }
        Pure::And(k) | Pure::Or(k) => {
            let (a, ka) = specialize(&k[0], kinds, code, d)?;
            if ka != Kind::B {
                return None;
            }
            let at = code.len();
            code.push(TIns {
                op: TOp::Mov,
                a,
                b: Src::K(0),
                dst: d,
            });
            let (b, kb) = specialize(&k[1], kinds, code, d)?;
            if kb != Kind::B {
                return None;
            }
            if !matches!(b, Src::R(r) if r == d) {
                code.push(TIns {
                    op: TOp::Mov,
                    a: b,
                    b: Src::K(0),
                    dst: d,
                });
            }
            let n = (code.len() - at - 1) as u32;
            code[at].op = match p {
                Pure::And(_) => TOp::And(n),
                _ => TOp::Or(n),
            };
            (Src::R(d), Kind::B)
        }
    })
}

/// Whether `op` reads its `b` operand.
fn reads_b(op: &TOp) -> bool {
    !matches!(
        op,
        TOp::And(_) | TOp::Or(_) | TOp::Mov | TOp::Load(..) | TOp::Br(_) | TOp::Jmp(_)
    )
}

/// The arrays a [`TOp::Load`]/[`TOp::Store`] reads and writes.
trait Mem {
    /// Element `i` of array `arr` when it is in bounds and of kind `k`.
    fn load(&self, arr: u8, i: u64, k: Kind) -> Option<u64>;
    /// Write element `i` of array `arr`, in bounds and of kind `k` now.
    fn store(&mut self, arr: u8, i: u64, x: u64, k: Kind) -> Option<()>;
    /// The iteration's element writes stand.
    fn commit(&mut self) {}
    /// Undo the iteration's element writes, last first.
    fn rollback(&mut self) {}
}

/// No arrays (code without [`TOp::Load`]/[`TOp::Store`]).
struct NoMem;

impl Mem for NoMem {
    fn load(&self, _: u8, _: u64, _: Kind) -> Option<u64> {
        None
    }
    fn store(&mut self, _: u8, _: u64, _: u64, _: Kind) -> Option<()> {
        None
    }
}

/// Run register code over an `N`-register file (`N` a power of two, so the
/// masked indexes need no bounds checks). `None` where an instruction
/// declines.
#[inline(always)]
fn exec<const N: usize, M: Mem>(code: &[RIns], r: &mut [u64; N], mem: &mut M) -> Option<()> {
    let m = N - 1;
    let f = f64::from_bits;
    let mut i = 0;
    while let Some(ins) = code.get(i) {
        let a = r[ins.a as usize & m];
        let b = r[ins.b as usize & m];
        let (ai, bi) = (a as i64, b as i64);
        let v = match &ins.op {
            TOp::AddI => ai.checked_add(bi)? as u64,
            TOp::SubI => ai.checked_sub(bi)? as u64,
            TOp::MulI => ai.checked_mul(bi)? as u64,
            TOp::LtI => (ai < bi) as u64,
            TOp::LeI => (ai <= bi) as u64,
            TOp::GtI => (ai > bi) as u64,
            TOp::GeI => (ai >= bi) as u64,
            TOp::EqI => (a == b) as u64,
            TOp::NeI => (a != b) as u64,
            TOp::OtherI(op) => match int_fast(op, ai, bi)? {
                Scalar::Int(n) => n as u64,
                Scalar::Bool(x) => x as u64,
                Scalar::Float(_) => return None,
            },
            TOp::AddF => (f(a) + f(b)).to_bits(),
            TOp::SubF => (f(a) - f(b)).to_bits(),
            TOp::MulF => (f(a) * f(b)).to_bits(),
            TOp::DivF => (f(a) / f(b)).to_bits(),
            TOp::LtF => (f(a) < f(b)) as u64,
            TOp::LeF => (f(a) <= f(b)) as u64,
            TOp::GtF => (f(a) > f(b)) as u64,
            TOp::GeF => (f(a) >= f(b)) as u64,
            TOp::EqF => (f(a) == f(b)) as u64,
            TOp::NeF => (f(a) != f(b)) as u64,
            TOp::OtherF(op) => match float_fast(op, f(a), f(b))? {
                Scalar::Float(x) => x.to_bits(),
                _ => return None,
            },
            TOp::Load(arr, k) => mem.load(*arr, a, *k)?,
            TOp::Store(arr, k) => {
                mem.store(*arr, a, b, *k)?;
                i += 1;
                continue;
            }
            TOp::Br(n) => {
                i += if a == 0 { *n as usize + 1 } else { 1 };
                continue;
            }
            TOp::Jmp(n) => {
                i += *n as usize + 1;
                continue;
            }
            TOp::And(n) => {
                if a == 0 {
                    i += *n as usize;
                }
                a
            }
            TOp::Or(n) => {
                if a != 0 {
                    i += *n as usize;
                }
                a
            }
            TOp::Mov => a,
        };
        r[ins.dst as usize & m] = v;
        i += 1;
    }
    Some(())
}

/// A local's register in [`Typed`]: read and kind-checked on every run.
struct Load {
    s: Sym,
    slot: u32,
    kind: Kind,
    reg: u8,
}

/// A pure tree's register code: locals and literals in registers
/// `0..LEAVES`, temporaries above.
pub(super) struct Typed {
    loads: Box<[Load]>,
    init: [u64; 16],
    code: Box<[RIns]>,
    res: u8,
    out: Kind,
}

impl Typed {
    /// Allocate `code`'s registers; `None` past [`LEAVES`] leaf registers.
    fn new(code: Vec<TIns>, res: Src, out: Kind) -> Option<Typed> {
        let mut loads: Vec<Load> = Vec::new();
        let mut init = [0u64; 16];
        let mut next = 0u8;
        let mut reg = |s: Src, loads: &mut Vec<Load>| -> Option<u8> {
            match s {
                Src::R(d) => return Some(LEAVES + d),
                Src::L(s, slot, kind) => {
                    if let Some(l) = loads.iter().find(|l| (l.s, l.slot) == (s, slot)) {
                        return Some(l.reg);
                    }
                    if next >= LEAVES {
                        return None;
                    }
                    loads.push(Load {
                        s,
                        slot,
                        kind,
                        reg: next,
                    });
                }
                Src::K(b) => {
                    if next >= LEAVES {
                        return None;
                    }
                    init[next as usize] = b;
                }
            }
            next += 1;
            Some(next - 1)
        };
        let mut rcode = Vec::with_capacity(code.len());
        for ins in code {
            let a = reg(ins.a, &mut loads)?;
            let b = if reads_b(&ins.op) {
                reg(ins.b, &mut loads)?
            } else {
                0
            };
            rcode.push(RIns {
                op: ins.op,
                a,
                b,
                dst: LEAVES + ins.dst,
            });
        }
        let res = reg(res, &mut loads)?;
        Some(Typed {
            loads: loads.into_boxed_slice(),
            init,
            code: rcode.into_boxed_slice(),
            res,
            out,
        })
    }

    /// Run against `env`'s locals; `None` on a decline (a local unbound or
    /// of another kind than the code was specialised on included).
    #[inline(always)]
    fn run(&self, env: &Env) -> Option<Scalar> {
        let mut r = self.init;
        for l in &self.loads[..] {
            r[l.reg as usize & 15] = bits(env.get_var(l.s, l.slot)?, l.kind)?;
        }
        exec(&self.code, &mut r, &mut NoMem)?;
        Some(scalar(r[self.res as usize & 15], self.out))
    }
}

/// A pure tree and its register code, built on the first run that asks for
/// it (`None` inside when those kinds do not fit it).
pub(in crate::interp) struct PureExpr<'p> {
    tree: Pure<'p>,
    typed: OnceCell<Option<Typed>>,
}

impl<'p> PureExpr<'p> {
    pub(super) fn new(tree: Pure<'p>) -> PureExpr<'p> {
        PureExpr {
            tree,
            typed: OnceCell::new(),
        }
    }

    /// The register code, specialised on `kinds` the first time.
    fn typed(&self, kinds: &dyn Fn(Sym, u32) -> Option<Kind>) -> Option<&Typed> {
        self.typed
            .get_or_init(|| {
                let mut code = Vec::new();
                let kinds = Kinds {
                    local: kinds,
                    elem: &no_elem,
                };
                let (res, out) = specialize(&self.tree, &kinds, &mut code, 0)?;
                Typed::new(code, res, out)
            })
            .as_ref()
    }

    /// The tree's value against `env`, or `None` (declined).
    #[inline(always)]
    pub(super) fn eval(&self, env: &Env) -> Option<Scalar> {
        match self.typed.get() {
            Some(Some(t)) => t.run(env),
            Some(None) => pure_eval(&self.tree, env),
            None => self.first_eval(env),
        }
    }

    #[cold]
    #[inline(never)]
    fn first_eval(&self, env: &Env) -> Option<Scalar> {
        self.typed(&|s, slot| env.get_var(s, slot).and_then(kind_of));
        pure_eval(&self.tree, env)
    }
}

/// `p`'s value against `env` by walking the tree, or `None` where the spec's
/// decline rules say so (module docs). The reference for those rules.
pub(super) fn pure_eval(p: &Pure<'_>, env: &Env) -> Option<Scalar> {
    Some(match p {
        Pure::Local(v) => match env.get_var(v.s, v.slot)? {
            Value::Int(n) => Scalar::Int(*n),
            Value::Float(f) => Scalar::Float(*f),
            Value::Bool(b) => Scalar::Bool(*b),
            _ => return None,
        },
        Pure::Int(n) => Scalar::Int(*n),
        Pure::Float(f) => Scalar::Float(*f),
        Pure::Bool(b) => Scalar::Bool(*b),
        Pure::Index(v, i) => {
            let Scalar::Int(i) = pure_eval(i, env)? else {
                return None;
            };
            let Value::Array(xs) = env.get_var(v.s, v.slot)? else {
                return None;
            };
            match xs.get(usize::try_from(i).ok()?)? {
                Value::Int(n) => Scalar::Int(*n),
                Value::Float(f) => Scalar::Float(*f),
                Value::Bool(b) => Scalar::Bool(*b),
                _ => return None,
            }
        }
        Pure::Bin(op, k) => match (pure_eval(&k[0], env)?, pure_eval(&k[1], env)?) {
            (Scalar::Int(a), Scalar::Int(b)) => int_fast(op, a, b)?,
            (Scalar::Float(a), Scalar::Float(b)) => float_fast(op, a, b)?,
            // `eval_binop_vals` (value.rs:719-720); every other operator on
            // two bools panics on the tree.
            (Scalar::Bool(a), Scalar::Bool(b)) => match op {
                BinOp::Eq => Scalar::Bool(a == b),
                BinOp::NotEq => Scalar::Bool(a != b),
                _ => return None,
            },
            _ => return None,
        },
        Pure::And(k) => match pure_eval(&k[0], env)? {
            Scalar::Bool(false) => Scalar::Bool(false),
            Scalar::Bool(true) => Scalar::Bool(pure_bool(&k[1], env)?),
            _ => return None,
        },
        Pure::Or(k) => match pure_eval(&k[0], env)? {
            Scalar::Bool(true) => Scalar::Bool(true),
            Scalar::Bool(false) => Scalar::Bool(pure_bool(&k[1], env)?),
            _ => return None,
        },
    })
}

/// [`pure_eval`] of a `&&`/`||` right side, which must be a bool.
fn pure_bool(p: &Pure<'_>, env: &Env) -> Option<bool> {
    match pure_eval(p, env)? {
        Scalar::Bool(b) => Some(b),
        _ => None,
    }
}

/// The locals `p` reads, in tree order (repeats included); an `xs[i]`
/// reads `i`'s locals here and `xs` through [`pure_arrays`].
fn pure_locals(p: &Pure<'_>, out: &mut Vec<(Sym, u32)>) {
    match p {
        Pure::Local(v) => out.push((v.s, v.slot)),
        Pure::Int(_) | Pure::Float(_) | Pure::Bool(_) => {}
        Pure::Index(_, i) => pure_locals(i, out),
        Pure::Bin(_, k) | Pure::And(k) | Pure::Or(k) => {
            pure_locals(&k[0], out);
            pure_locals(&k[1], out);
        }
    }
}

/// The arrays `p` indexes, in tree order (repeats included).
fn pure_arrays(p: &Pure<'_>, out: &mut Vec<(Sym, u32)>) {
    match p {
        Pure::Local(_) | Pure::Int(_) | Pure::Float(_) | Pure::Bool(_) => {}
        Pure::Index(v, i) => {
            out.push((v.s, v.slot));
            pure_arrays(i, out);
        }
        Pure::Bin(_, k) | Pure::And(k) | Pure::Or(k) => {
            pure_arrays(&k[0], out);
            pure_arrays(&k[1], out);
        }
    }
}

/// One statement of a [`PureLoop`] body (`Compiler::pure_stmts`).
pub(super) enum PureStmt<'p> {
    /// `let var = value` (`is_let`, top level only) or `var = value`.
    Set {
        var: Var<'p>,
        is_let: bool,
        value: Pure<'p>,
    },
    /// `arr[idx] = value`.
    Store {
        arr: Var<'p>,
        idx: Pure<'p>,
        value: Pure<'p>,
    },
    /// `if cond { then } else { else_ }`, at most [`PURE_DEPTH`] deep.
    If {
        cond: Pure<'p>,
        then: Box<[PureStmt<'p>]>,
        else_: Box<[PureStmt<'p>]>,
    },
}

/// Every statement of `body`, branches included, in source order.
fn each_stmt<'a, 'p>(body: &'a [PureStmt<'p>], f: &mut dyn FnMut(&'a PureStmt<'p>)) {
    for st in body {
        f(st);
        if let PureStmt::If { then, else_, .. } = st {
            each_stmt(then, f);
            each_stmt(else_, f);
        }
    }
}

/// A `while` whose condition is a pure tree, or a `for` over an integer
/// range, whose body is pure statements, run in registers (spec §4 S7,
/// S10). On entry the env locals it reads or assigns (not the body's own
/// `let`s or the `for` variable) are read once with the kind check, and the
/// arrays it indexes are taken out of the env; each iteration runs the
/// condition (or the counter test), then the statements, in registers; a
/// body `let` and the `for` variable are registers only and the iteration's
/// scope is never pushed. An element write copies a shared array first, as
/// the tree's does. When the loop ends the assigned locals and the arrays
/// are written back once. When any step declines, the registers go back to
/// the iteration's start, its element writes are undone, everything is
/// written back, and the generic loop re-runs that iteration (its panic or
/// slow path exactly). Nothing in the region can see the `Env` between
/// those points.
pub(in crate::interp) struct PureLoop<'p> {
    /// The `while` condition; `None` for a `for`.
    cond: Option<Pure<'p>>,
    /// The `for` variable and whether its range is inclusive.
    for_: Option<(Var<'p>, bool)>,
    body: Box<[PureStmt<'p>]>,
    code: OnceCell<Option<LoopCode>>,
}

/// A local of the env a [`PureLoop`] holds in register `i` (its index).
struct LoopVar {
    s: Sym,
    slot: u32,
    kind: Kind,
    assigned: bool,
}

/// An array local a [`PureLoop`] indexes, its first element's kind the one
/// [`TOp::Load`] reads it as.
struct LoopArr {
    s: Sym,
    slot: u32,
    kind: Kind,
}

/// Env locals in registers `0..8`, body `let`s (the `for` variable first)
/// `8..16`, literals `16..24`, temporaries `24..32`.
const LOOP_LETS: u8 = 8;
const LOOP_LITS: u8 = 16;
const LOOP_TEMPS: u8 = 24;
/// The most arrays one loop indexes.
const LOOP_ARRS: usize = 4;

/// A [`PureLoop`]'s register code, specialised on the kinds of its first
/// entry.
struct LoopCode {
    vars: Box<[LoopVar]>,
    arrs: Box<[LoopArr]>,
    init: [u64; 32],
    cond: Box<[RIns]>,
    cres: u8,
    body: Box<[RIns]>,
}

/// The arrays of a running [`PureLoop`], taken out of the env, and the
/// element writes of the current iteration (array, index, old element's
/// bits). A write needs the old element to be a scalar of the store's kind
/// (else it declines), so undoing one is a write of the same kind.
struct LoopMem {
    arrs: [Option<Rc<Elems>>; LOOP_ARRS],
    undo: Vec<(u8, usize, u64)>,
}

/// Overwrite the scalar `v` (of kind `bits` accepted) with bits `x`.
#[inline(always)]
fn put_bits(v: &mut Value, x: u64) {
    match v {
        Value::Int(n) => *n = x as i64,
        Value::Float(f) => *f = f64::from_bits(x),
        Value::Bool(b) => *b = x != 0,
        _ => unreachable!("vm: a pure loop's element changed kind"),
    }
}

impl LoopMem {
    /// Array `arr` for writing: unique (copied first when shared, as the
    /// tree's element write does).
    #[inline(always)]
    fn items_mut(&mut self, arr: u8) -> Option<&mut Vec<Value>> {
        let items = self.arrs[arr as usize % LOOP_ARRS].as_mut()?;
        if Rc::get_mut(items).is_none() {
            return Some(&mut **Rc::make_mut(items));
        }
        Rc::get_mut(items).map(|e| &mut **e)
    }
}

impl Mem for LoopMem {
    #[inline(always)]
    fn load(&self, arr: u8, i: u64, k: Kind) -> Option<u64> {
        let items = self.arrs[arr as usize % LOOP_ARRS].as_ref()?;
        bits(items.get(usize::try_from(i).ok()?)?, k)
    }

    #[inline(always)]
    fn store(&mut self, arr: u8, i: u64, x: u64, k: Kind) -> Option<()> {
        let i = usize::try_from(i).ok()?;
        let items = self.arrs[arr as usize % LOOP_ARRS].as_ref()?;
        let old = bits(items.get(i)?, k)?;
        put_bits(&mut self.items_mut(arr)?[i], x);
        self.undo.push((arr, i, old));
        Some(())
    }

    #[inline(always)]
    fn commit(&mut self) {
        self.undo.clear();
    }

    fn rollback(&mut self) {
        while let Some((arr, i, old)) = self.undo.pop() {
            if let Some(items) = self.items_mut(arr) {
                put_bits(&mut items[i], old);
            }
        }
    }
}

impl<'p> PureLoop<'p> {
    pub(super) fn new(
        cond: Option<Pure<'p>>,
        for_: Option<(Var<'p>, bool)>,
        body: Vec<PureStmt<'p>>,
    ) -> PureLoop<'p> {
        PureLoop {
            cond,
            for_,
            body: body.into_boxed_slice(),
            code: OnceCell::new(),
        }
    }

    /// Run the `while` loop against `env`: `true` when its condition went
    /// false (the loop is done), `false` when the generic loop must run from
    /// its condition (the env holds the current iteration's start).
    #[inline(never)]
    pub(super) fn run(&self, env: &mut Env) -> bool {
        self.drive(env, None)
    }

    /// Run the `for` loop from counter `i` to bound `e`: `true` when the
    /// counter passed the bound, `false` when the generic loop must run the
    /// iteration `i` now holds (the env holds that iteration's start).
    #[inline(never)]
    pub(super) fn run_for(&self, env: &mut Env, i: &mut i64, e: i64) -> bool {
        self.drive(env, Some((i, e)))
    }

    #[inline(always)]
    fn drive(&self, env: &mut Env, ctr: Option<(&mut i64, i64)>) -> bool {
        let Some(t) = self.code.get_or_init(|| self.build(env)) else {
            return false;
        };
        let mut r = t.init;
        for (i, v) in t.vars.iter().enumerate() {
            match env.get_var(v.s, v.slot).and_then(|x| bits(x, v.kind)) {
                Some(b) => r[i & 31] = b,
                None => return false,
            }
        }
        let mut mem = LoopMem {
            arrs: Default::default(),
            undo: Vec::new(),
        };
        for (a, slot) in t.arrs.iter().zip(&mut mem.arrs) {
            match env.get_var_mut(a.s, a.slot) {
                Some(v @ Value::Array(_)) => {
                    let Value::Array(items) = std::mem::replace(v, Value::Unit) else {
                        unreachable!()
                    };
                    *slot = Some(items);
                }
                _ => {
                    put_arrays(env, &t.arrs, &mut mem);
                    return false;
                }
            }
        }
        // A loop that indexes no array runs without the undo log (`NoMem`).
        let done = if t.arrs.is_empty() {
            self.iterate(t, &mut r, ctr, &mut NoMem)
        } else {
            self.iterate(t, &mut r, ctr, &mut mem)
        };
        put_arrays(env, &t.arrs, &mut mem);
        for (i, v) in t.vars.iter().enumerate() {
            if !v.assigned {
                continue;
            }
            let x = r[i & 31];
            match env.get_var_mut(v.s, v.slot) {
                Some(Value::Int(d)) => *d = x as i64,
                Some(Value::Float(d)) => *d = f64::from_bits(x),
                Some(Value::Bool(d)) => *d = x != 0,
                // Entry read this binding with this kind, and nothing in
                // the region can rebind it.
                _ => unreachable!("vm: a pure loop's local changed kind"),
            }
        }
        done
    }

    /// Run iterations in registers until the loop ends (`true`) or a step
    /// declines (`false`, the registers and `mem` back at that iteration's
    /// start).
    #[inline(always)]
    fn iterate<M: Mem>(
        &self,
        t: &LoopCode,
        r: &mut [u64; 32],
        mut ctr: Option<(&mut i64, i64)>,
        mem: &mut M,
    ) -> bool {
        let inclusive = matches!(self.for_, Some((_, true)));
        loop {
            if let Some((i, e)) = &ctr {
                let i = **i;
                if !(if inclusive { i <= *e } else { i < *e }) {
                    return true;
                }
                // The generic `ForNext` increments past it.
                if i == i64::MAX {
                    return false;
                }
                r[LOOP_LETS as usize] = i as u64;
            } else {
                if exec(&t.cond, r, mem).is_none() {
                    return false;
                }
                if r[t.cres as usize & 31] == 0 {
                    return true;
                }
            }
            let mut start = [0u64; LOOP_LETS as usize];
            start.copy_from_slice(&r[..LOOP_LETS as usize]);
            if exec(&t.body, r, mem).is_none() {
                r[..LOOP_LETS as usize].copy_from_slice(&start);
                mem.rollback();
                return false;
            }
            mem.commit();
            if let Some((i, _)) = &mut ctr {
                **i += 1;
            }
        }
    }

    /// The register code for the kinds `env`'s locals hold now; `None` when
    /// a first run would decline on them (a local unbound or not a plain
    /// scalar, an array local unbound, empty or of non-scalar elements, a
    /// statement whose kind differs from its register's, a body `let` read
    /// before it is bound, too many registers or arrays).
    #[cold]
    fn build(&self, env: &Env) -> Option<LoopCode> {
        let mut lets: Vec<(Sym, u32)> = Vec::new();
        if let Some((v, _)) = &self.for_ {
            lets.push((v.s, v.slot));
        }
        for st in &self.body[..] {
            if let PureStmt::Set {
                var, is_let: true, ..
            } = st
            {
                if !lets.contains(&(var.s, var.slot)) {
                    lets.push((var.s, var.slot));
                }
            }
        }
        let mut reads = Vec::new();
        let mut arr_reads = Vec::new();
        let mut assigned = Vec::new();
        if let Some(c) = &self.cond {
            pure_locals(c, &mut reads);
            pure_arrays(c, &mut arr_reads);
        }
        each_stmt(&self.body, &mut |st| match st {
            PureStmt::Set { var, is_let, value } => {
                pure_locals(value, &mut reads);
                pure_arrays(value, &mut arr_reads);
                reads.push((var.s, var.slot));
                if !is_let {
                    assigned.push((var.s, var.slot));
                }
            }
            PureStmt::Store { arr, idx, value } => {
                for p in [idx, value] {
                    pure_locals(p, &mut reads);
                    pure_arrays(p, &mut arr_reads);
                }
                arr_reads.push((arr.s, arr.slot));
            }
            PureStmt::If { cond, .. } => {
                pure_locals(cond, &mut reads);
                pure_arrays(cond, &mut arr_reads);
            }
        });
        let mut vars: Vec<LoopVar> = Vec::new();
        for (s, slot) in reads {
            if lets.contains(&(s, slot)) || vars.iter().any(|v| (v.s, v.slot) == (s, slot)) {
                continue;
            }
            let kind = kind_of(env.get_var(s, slot)?)?;
            vars.push(LoopVar {
                s,
                slot,
                kind,
                assigned: assigned.contains(&(s, slot)),
            });
        }
        let mut arrs: Vec<LoopArr> = Vec::new();
        for (s, slot) in arr_reads {
            if arrs.iter().any(|a| (a.s, a.slot) == (s, slot)) {
                continue;
            }
            let Value::Array(items) = env.get_var(s, slot)? else {
                return None;
            };
            let kind = kind_of(items.first()?)?;
            arrs.push(LoopArr { s, slot, kind });
        }
        // Two bindings of one name (a body `let` shadowing a local, say):
        // one name must stand for one register, since `get_var` falls back
        // to the name when the env is not laid out as resolution assumed.
        let mut names: Vec<Sym> = Vec::new();
        let all = vars.iter().map(|v| v.s);
        for s in all
            .chain(lets.iter().map(|l| l.0))
            .chain(arrs.iter().map(|a| a.s))
        {
            if names.contains(&s) {
                return None;
            }
            names.push(s);
        }
        if vars.len() > LOOP_LETS as usize
            || lets.len() > (LOOP_LITS - LOOP_LETS) as usize
            || arrs.len() > LOOP_ARRS
        {
            return None;
        }
        let mut b = LoopBuilder {
            vars: &vars,
            arrs: &arrs,
            lets: &lets,
            bound: Vec::new(),
            init: [0u64; 32],
            lits: 0,
        };
        if let Some((v, _)) = &self.for_ {
            b.bound.push(((v.s, v.slot), Kind::I));
        }
        let mut cond = Vec::new();
        let cres = match &self.cond {
            Some(c) => match b.lower_kind(c, &mut cond, 0)? {
                (r, Kind::B) => r,
                _ => return None,
            },
            None => 0,
        };
        let mut body = Vec::new();
        b.lower_stmts(&self.body, &mut body)?;
        let init = b.init;
        Some(LoopCode {
            vars: vars.into_boxed_slice(),
            arrs: arrs.into_boxed_slice(),
            init,
            cond: cond.into_boxed_slice(),
            cres,
            body: body.into_boxed_slice(),
        })
    }
}

/// Give the arrays [`PureLoop::drive`] took back to `env`.
fn put_arrays(env: &mut Env, arrs: &[LoopArr], mem: &mut LoopMem) {
    for (a, slot) in arrs.iter().zip(&mut mem.arrs) {
        if let Some(items) = slot.take() {
            match env.get_var_mut(a.s, a.slot) {
                Some(v) => *v = Value::Array(items),
                // Taken from this binding, and nothing in the region can
                // unbind it.
                None => unreachable!("vm: a pure loop's array lost its binding"),
            }
        }
    }
}

/// Register allocation for a [`LoopCode`]; `bound` lists the body `let`s
/// bound so far in statement order (the `for` variable first), with their
/// kinds.
struct LoopBuilder<'a> {
    vars: &'a [LoopVar],
    arrs: &'a [LoopArr],
    lets: &'a [(Sym, u32)],
    bound: Vec<((Sym, u32), Kind)>,
    init: [u64; 32],
    lits: u8,
}

impl LoopBuilder<'_> {
    fn let_reg(&self, key: (Sym, u32)) -> Option<u8> {
        let i = self.lets.iter().position(|l| *l == key)?;
        Some(LOOP_LETS + i as u8)
    }

    /// The kind of local `(s, slot)` at this point of the body: its entry
    /// kind, or the kind of the body `let` that bound it.
    fn kind(&self, s: Sym, slot: u32) -> Option<Kind> {
        if let Some(v) = self.vars.iter().find(|v| (v.s, v.slot) == (s, slot)) {
            return Some(v.kind);
        }
        self.bound.iter().find(|l| l.0 == (s, slot)).map(|l| l.1)
    }

    /// Lower `stmts` into `out`.
    fn lower_stmts(&mut self, stmts: &[PureStmt<'_>], out: &mut Vec<RIns>) -> Option<()> {
        for st in stmts {
            match st {
                PureStmt::Set { var, is_let, value } => {
                    self.lower_set((var.s, var.slot), *is_let, value, out)?
                }
                PureStmt::Store { arr, idx, value } => {
                    let a = self
                        .arrs
                        .iter()
                        .position(|a| (a.s, a.slot) == (arr.s, arr.slot))?;
                    let (ri, ki) = self.lower_kind(idx, out, 0)?;
                    if ki != Kind::I {
                        return None;
                    }
                    // Temporaries from 1: the index's stays live.
                    let (rv, kv) = self.lower_kind(value, out, 1)?;
                    out.push(RIns {
                        op: TOp::Store(a as u8, kv),
                        a: ri,
                        b: rv,
                        dst: 0,
                    });
                }
                PureStmt::If { cond, then, else_ } => {
                    let (rc, kc) = self.lower_kind(cond, out, 0)?;
                    if kc != Kind::B {
                        return None;
                    }
                    let br = out.len();
                    out.push(RIns {
                        op: TOp::Br(0),
                        a: rc,
                        b: 0,
                        dst: 0,
                    });
                    self.lower_stmts(then, out)?;
                    if else_.is_empty() {
                        out[br].op = TOp::Br((out.len() - br - 1) as u32);
                    } else {
                        let jmp = out.len();
                        out.push(RIns {
                            op: TOp::Jmp(0),
                            a: 0,
                            b: 0,
                            dst: 0,
                        });
                        out[br].op = TOp::Br((jmp - br) as u32);
                        self.lower_stmts(else_, out)?;
                        out[jmp].op = TOp::Jmp((out.len() - jmp - 1) as u32);
                    }
                }
            }
        }
        Some(())
    }

    /// Lower `let key = value` (`is_let`) or `key = value` into `out`.
    fn lower_set(
        &mut self,
        key: (Sym, u32),
        is_let: bool,
        value: &Pure<'_>,
        out: &mut Vec<RIns>,
    ) -> Option<()> {
        let at = out.len();
        let (src, kind) = self.lower_kind(value, out, 0)?;
        let dst = if is_let {
            match self.bound.iter().find(|x| x.0 == key) {
                Some(x) if x.1 != kind => return None,
                Some(_) => {}
                None => self.bound.push((key, kind)),
            }
            self.let_reg(key)?
        } else if let Some(i) = self.vars.iter().position(|v| (v.s, v.slot) == key) {
            if self.vars[i].kind != kind {
                return None;
            }
            i as u8
        } else {
            // An assignment to a body `let` bound earlier, or to the `for`
            // variable.
            let x = self.bound.iter().find(|x| x.0 == key)?;
            if x.1 != kind {
                return None;
            }
            self.let_reg(key)?
        };
        // The value's last instruction can write the destination itself
        // (every read of the destination comes before it), unless a
        // `&&`/`||` may skip that instruction.
        let plain = out.len() > at
            && !out[at..]
                .iter()
                .any(|i: &RIns| matches!(i.op, TOp::And(_) | TOp::Or(_)));
        match out.last_mut() {
            Some(last) if plain && last.dst == src => last.dst = dst,
            _ => out.push(RIns {
                op: TOp::Mov,
                a: src,
                b: 0,
                dst,
            }),
        }
        Some(())
    }

    /// Lower `p` into `out`, its temporaries from `d`; its value's register
    /// and kind.
    fn lower_kind(&mut self, p: &Pure<'_>, out: &mut Vec<RIns>, d: u8) -> Option<(u8, Kind)> {
        let mut code = Vec::new();
        let kinds = Kinds {
            local: &|s, slot| self.kind(s, slot),
            elem: &|s, slot| {
                let i = self.arrs.iter().position(|a| (a.s, a.slot) == (s, slot))?;
                Some((i as u8, self.arrs[i].kind))
            },
        };
        let (res, kind) = specialize(p, &kinds, &mut code, d)?;
        for ins in code {
            let a = self.reg(ins.a)?;
            let b = if reads_b(&ins.op) {
                self.reg(ins.b)?
            } else {
                0
            };
            out.push(RIns {
                op: ins.op,
                a,
                b,
                dst: LOOP_TEMPS + ins.dst,
            });
        }
        Some((self.reg(res)?, kind))
    }

    fn reg(&mut self, s: Src) -> Option<u8> {
        Some(match s {
            Src::R(d) => LOOP_TEMPS + d,
            Src::L(s, slot, _) => {
                if let Some(i) = self.vars.iter().position(|v| (v.s, v.slot) == (s, slot)) {
                    return Some(i as u8);
                }
                // A body `let`, readable once bound (a read before that is
                // of a binding the env holds: declined at entry).
                self.bound.iter().find(|l| l.0 == (s, slot))?;
                self.let_reg((s, slot))?
            }
            Src::K(b) => {
                let lits = &self.init[LOOP_LITS as usize..(LOOP_LITS + self.lits) as usize];
                if let Some(i) = lits.iter().position(|x| *x == b) {
                    return Some(LOOP_LITS + i as u8);
                }
                if self.lits >= LOOP_TEMPS - LOOP_LITS {
                    return None;
                }
                self.init[(LOOP_LITS + self.lits) as usize] = b;
                self.lits += 1;
                LOOP_LITS + self.lits - 1
            }
        })
    }
}

impl Interp<'_> {
    /// R50 S7 `fold_leaf` (spec §4 S7): `arr_fold`'s later elements `xs`
    /// (after the first or second, once a general call compiled the closure's
    /// body; 0 while it is not compiled) in
    /// registers, when the engine is `vm` and `f` is a closure of two
    /// parameters whose compiled body is one pure tree over them, its
    /// captures and literals. The captures are read once (a pure body cannot
    /// write them), the accumulator stays in a register, and each element
    /// gets the kind check. Returns how many of `xs` it folded into `acc`;
    /// it stops before an element whose step declines, which with every
    /// later one the caller runs through the general call (its panic
    /// included). Skipping the closure frame is unobservable: a lambda call
    /// binds its parameters without coercion and has no depth guard, and a
    /// pure body neither writes its captures nor calls out.
    pub(in crate::interp) fn fold_leaf(&self, f: &Value, xs: &[Value], acc: &mut Value) -> usize {
        if self.engine != Engine::Vm || xs.is_empty() {
            return 0;
        }
        let Value::Closure(cv) = f else {
            return 0;
        };
        let code = &*cv.code;
        let (Some(lb), Some(pb), [p0, p1]) = (&code.compiled, code.param_base, &code.params[..])
        else {
            return 0;
        };
        let Some(leaf) = lb.compiled().and_then(|b| b.leaf.as_deref()) else {
            return 0;
        };
        let (p0, p1) = (*p0, *p1);
        let cell = cv.captured.borrow();
        if cell.len() != pb as usize {
            return 0;
        }
        // The frame a general call builds: the captures at slots `0..pb`,
        // then the parameters.
        let value_at = |s: Sym, slot: u32| -> Option<&Value> {
            match slot.checked_sub(pb) {
                Some(0) => (s == p0).then_some(&*acc),
                Some(1) => (s == p1).then_some(&xs[0]),
                Some(_) => None,
                None => cell.get(slot as usize).filter(|c| c.0 == s).map(|c| &c.1),
            }
        };
        let Some(t) = leaf.typed(&|s, slot| value_at(s, slot).and_then(kind_of)) else {
            return 0;
        };
        let mut r = t.init;
        let (mut ra, mut rx) = (None, None);
        let mut kx = Kind::I;
        for l in &t.loads[..] {
            match l.slot.checked_sub(pb) {
                Some(0) if l.s == p0 && l.kind == t.out => ra = Some(l.reg as usize & 15),
                Some(1) if l.s == p1 => (rx, kx) = (Some(l.reg as usize & 15), l.kind),
                None => match value_at(l.s, l.slot).and_then(|v| bits(v, l.kind)) {
                    Some(b) => r[l.reg as usize & 15] = b,
                    None => return 0,
                },
                _ => return 0,
            }
        }
        drop(cell);
        let Some(mut a) = bits(acc, t.out) else {
            return 0;
        };
        if self.vm_trace && !lb.fold_traced.replace(true) {
            eprintln!("vm: fold-leaf {}", lb.name);
        }
        let mut n = 0;
        for x in xs {
            if let Some(rx) = rx {
                match bits(x, kx) {
                    Some(b) => r[rx] = b,
                    None => break,
                }
            }
            if let Some(ra) = ra {
                r[ra] = a;
            }
            if exec(&t.code, &mut r, &mut NoMem).is_none() {
                break;
            }
            a = r[t.res as usize & 15];
            n += 1;
        }
        if n > 0 {
            *acc = scalar(a, t.out).value();
        }
        n
    }
}
