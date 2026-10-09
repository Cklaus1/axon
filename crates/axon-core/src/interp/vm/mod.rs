//! R50: the bytecode engine for fn bodies (`governance/specs/R50-register-vm.md`).
//!
//! A fn body in `Interp::fn_table` is compiled on its first run under
//! `AXON_ENGINE=vm` into a [`Body`]: a flat op sequence that runs against the
//! same `Env` the tree-walker would use (spec §4 Fork 1), so params, `goal_met`,
//! `&mut` read-back, postconditions and closure capture see the bindings where
//! they always were. A node the compiler does not lower is one [`Op::Tree`],
//! which calls [`Interp::eval`] on it (§4 Fork 2). Slice S1 lowers the scalar
//! core: literals, local reads, blocks, `let`, `=` to a local, operators,
//! `if`/`while`/`for`, `return`/`break`/`continue`/`?`, `Some`/`None`/`Ok`/
//! `Err`, string interpolation and calls by name (§4 "Lowered set per slice").
//!
//! Values flow through an operand stack owned by the activation (kept in its
//! frame, `Env::stack`). An op that reads a local or a literal directly takes
//! it as an inline [`Opnd`] instead of a stack slot, and an assignment whose
//! value is a binary operation on two such operands is one [`Op::StoreBin`].
//!
//! The tree-walker stays the reference engine (I-2). `AXON_ENGINE=tree` (the
//! default) never compiles anything.

use super::eval::{
    binop_operands, cond_bool, logic_rhs, question, short_circuits, strict_int, Operand,
};
use super::eval::{field_of, index_value};
use super::*;
use crate::ast::{AxonType, UnaryOp};

mod compile;
mod pure;
#[cfg(test)]
mod tests;

pub(super) use compile::compile;
use pure::{PureExpr, PureLoop, PureOp};

/// Which engine runs fn bodies. Chosen once per `Interp` (spec §4 Activation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Engine {
    /// The tree-walker (`Interp::eval`), the reference engine.
    Tree,
    /// Compiled ops ([`Body`]) for fn-table bodies, the tree for the rest.
    Vm,
}

impl Engine {
    /// The engine when nothing selects one.
    pub const DEFAULT: Engine = Engine::Tree;

    /// Parse an `AXON_ENGINE` value; the error is the message the CLI prints
    /// before exiting 2 (spec §3).
    pub fn parse(raw: &str) -> Result<Engine, String> {
        match raw {
            "tree" => Ok(Engine::Tree),
            "vm" => Ok(Engine::Vm),
            other => Err(format!(
                "AXON_ENGINE must be \"vm\" or \"tree\" (got \"{other}\")"
            )),
        }
    }
}

/// [`set_engine`]'s choice: 0 = none (use `AXON_ENGINE`), 1 = tree, 2 = vm.
/// A process-wide atomic rather than a thread-local because native runs build
/// the `Interp` on the deep-stack thread (`on_deep_stack`).
static ENGINE_OVERRIDE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

/// Select the engine for every `Interp` built afterwards, overriding
/// `AXON_ENGINE`. For hosts without an environment: `axon-wasm`'s
/// `axon_set_engine` export on `wasm32-unknown-unknown` (spec §4 Activation).
pub fn set_engine(engine: Engine) {
    let code = match engine {
        Engine::Tree => 1,
        Engine::Vm => 2,
    };
    ENGINE_OVERRIDE.store(code, std::sync::atomic::Ordering::Relaxed);
}

/// The engine an `Interp` being built runs with: [`set_engine`]'s choice, else
/// `AXON_ENGINE`, else [`Engine::DEFAULT`]. An invalid `AXON_ENGINE` prints the
/// spec §3 message and exits 2. `Interp::build` runs before any of the
/// program does, under every entry (`axon run`, `axon-run`, `axon test`,
/// `axon goal`), so this is the one place that check needs.
pub(super) fn engine_at_build() -> Engine {
    match ENGINE_OVERRIDE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => return Engine::Tree,
        2 => return Engine::Vm,
        _ => {}
    }
    match std::env::var_os("AXON_ENGINE") {
        None => Engine::DEFAULT,
        Some(raw) => Engine::parse(&raw.to_string_lossy()).unwrap_or_else(|msg| {
            eprintln!("{msg}");
            std::process::exit(2)
        }),
    }
}

/// `AXON_VM_TRACE=1` (spec §3), read once when the `Interp` is built.
pub(super) fn trace_at_build() -> bool {
    std::env::var_os("AXON_VM_TRACE").is_some_and(|v| v == "1")
}

/// A compiled fn body. It borrows the nodes of the program it was compiled
/// from (`Tree` ops, names, operators), so it lives in that fn's
/// `FnEntry::compiled` and never outlives the program.
pub(super) struct Body<'p> {
    pub(super) ops: Box<[Op<'p>]>,
    /// The body's loops, each loop listed before every loop enclosing it, so
    /// the first one whose iteration range holds an op is the innermost.
    pub(super) loops: Box<[Loop]>,
    /// The operand-stack height the body reaches at most.
    pub(super) max_stack: usize,
    /// R50 S7: the body as one pure tree, when it is one (a lambda body
    /// such as `acc + x % m`), for [`Interp::fold_leaf`].
    pub(super) leaf: Option<Box<PureExpr<'p>>>,
}

impl Body<'_> {
    /// The number of [`Op::Tree`] ops, the trace's `<k> tree nodes`.
    pub(super) fn tree_nodes(&self) -> usize {
        self.ops
            .iter()
            .filter(|op| matches!(op, Op::Tree(_)))
            .count()
    }

    /// The [`Op::Pure`] and [`Op::PureLoop`] ops, the trace's `vm: pure`
    /// line's `<p> exprs, <q> loops`.
    fn pure_ops(&self) -> (usize, usize) {
        let p = self
            .ops
            .iter()
            .filter(|op| matches!(op, Op::Pure(_)))
            .count();
        let q = self
            .ops
            .iter()
            .filter(|op| matches!(op, Op::PureLoop { .. }))
            .count();
        (p, q)
    }

    /// The innermost loop of this body whose iteration holds op `at`: the
    /// one a `Flow::Break`/`Flow::Continue` out of that op belongs to, as
    /// `run_loop_body` catches them on the tree.
    fn loop_at(&self, at: u32) -> Option<&Loop> {
        self.loops.iter().find(|l| l.start <= at && at < l.end)
    }
}

/// R50 S4: a lambda body's compiled code, kept in its `ClosureCode`
/// (`compiled`) so it lives exactly as long as the body it was compiled from.
/// No compiled code is stored in the `Interp`.
pub(super) struct LambdaBody {
    /// The trace name, `<owner>::lambda#<i>` (spec §3).
    name: Box<str>,
    /// Filled on the body's first run under `AXON_ENGINE=vm`. Its ops point
    /// into the owning `ClosureCode::body` (`Tree` nodes, names, operands):
    /// `'static` stands for that borrow, which [`LambdaBody::get`] hands out
    /// only for as long as the code is borrowed. Dropping a `Body` reads
    /// none of those pointers.
    body: std::cell::OnceCell<Body<'static>>,
    /// Whether the `vm: fold-leaf` trace line was printed (spec §3: once
    /// per lambda code).
    fold_traced: std::cell::Cell<bool>,
}

impl LambdaBody {
    pub(super) fn new(name: String) -> LambdaBody {
        LambdaBody {
            name: name.into_boxed_str(),
            body: std::cell::OnceCell::new(),
            fold_traced: std::cell::Cell::new(false),
        }
    }

    /// The compiled body, once its first run compiled it.
    #[inline(always)]
    fn compiled(&self) -> Option<&Body<'_>> {
        self.body.get()
    }

    /// The compiled body of `code`, whose `compiled` this is, compiled by
    /// `init` on the first call.
    fn get<'a>(
        &'a self,
        code: &'a ClosureCode,
        init: impl FnOnce(&'a Expr) -> Body<'a>,
    ) -> &'a Body<'a> {
        debug_assert!(code
            .compiled
            .as_ref()
            .is_some_and(|c| std::ptr::eq(c, self)));
        self.body.get_or_init(|| {
            let body = init(&code.body);
            // SAFETY: `body` borrows only `code.body`, the body of the
            // `ClosureCode` that owns `self`. A `ClosureCode` is only ever
            // built inside an `Rc` and never moved out of it or mutated
            // (sym.rs), so those nodes stay where they are until the code is
            // dropped, and `self.body` is dropped with it. The extended
            // lifetime never escapes: `get` returns the body at `'a` again,
            // and `Body` is covariant in its lifetime.
            unsafe { std::mem::transmute::<Body<'a>, Body<'static>>(body) }
        })
    }
}

impl Body<'_> {
    /// R50 S4: the value of a body that is one int or float operation on
    /// locals and literals (`|acc, x| acc + x`), computed without an
    /// activation of the op loop by the scalar fast path its `Bin` op tries
    /// first (cost only). S5: also a body that reads one bound local
    /// (`fn id(x: i64) -> i64 { x }`), the value its `Load` op pushes. `None`
    /// for any other body, and wherever that fast path declines (another
    /// operand type, overflow, a zero divisor, an unbound name); the op loop
    /// then runs the body, panic included. With `pre_goal` (a fast call,
    /// S5) the frame holds only the parameters, `goal_met` not pushed yet:
    /// a body that names `goal_met` declines, so every name it reads finds
    /// the binding it would find with `goal_met` there.
    #[inline(always)]
    fn leaf_value(&self, env: &Env, pre_goal: bool) -> Option<Value> {
        let goal = |o: &Opnd<'_>| matches!(o, Opnd::Local(v) if v.s == SYM_GOAL_MET);
        let v = match &*self.ops {
            [Op::Bin { op, l, r }] => {
                if pre_goal && (goal(l) || goal(r)) {
                    return None;
                }
                // A one-op body has no stack operand; `scalar_at` declines one.
                match (scalar_at(l, env, &[], 0)?, scalar_at(r, env, &[], 0)?) {
                    (Scalar::Int(a), Scalar::Int(b)) => int_fast(op, a, b)?,
                    (Scalar::Float(a), Scalar::Float(b)) => float_fast(op, a, b)?,
                    _ => return None,
                }
            }
            [Op::Load(var)] => {
                if pre_goal && var.s == SYM_GOAL_MET {
                    return None;
                }
                return Some(match env.get_var(var.s, var.slot)? {
                    Value::Int(n) => Value::Int(*n),
                    v => v.clone(),
                });
            }
            _ => return None,
        };
        Some(v.value())
    }
}

/// A `while`, `while let` or `for` loop of a [`Body`]: where `break`/`continue` land.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Loop {
    /// Ops `start..end` are one iteration's statements (inside the scope the
    /// iteration pushes). The condition and the `for` bounds are outside: a
    /// `break` there belongs to an enclosing loop, as on the tree.
    pub(super) start: u32,
    pub(super) end: u32,
    /// Scopes the activation has pushed outside the iteration; a caught
    /// `break` pops back to this many, one at a time.
    pub(super) scopes: u32,
    /// Operand-stack height outside the iteration (a `for` keeps its counter
    /// and bound there).
    pub(super) height: u32,
    /// Where `break` continues: the loop's exit.
    pub(super) brk: u32,
    /// Where `continue` continues: the back edge (`while`), the
    /// [`Op::ForNext`] that ends the iteration (`for`).
    pub(super) cont: u32,
    /// The scopes still pushed where `continue` continues: the ones that op
    /// pops are still there (the iteration's, when it has one; a `for`'s
    /// variable scope).
    pub(super) cont_scopes: u32,
}

/// A local (or a name the resolver left to `globals`/fns) as an op names it:
/// the `(sym, slot)` the resolver gave the node, and its source name for the
/// slow paths' messages.
#[derive(Clone, Copy)]
pub(super) struct Var<'p> {
    pub(super) s: Sym,
    pub(super) slot: u32,
    pub(super) name: &'p String,
}

/// An operand of [`Op::Bin`], [`Op::StoreBin`] and [`Op::BranchCmp`]. Inline
/// operands are read when the op runs; the compiler only makes the left one
/// inline when the right one is too, so no code runs between where the tree
/// reads the left operand and where the op reads it.
pub(super) enum Opnd<'p> {
    /// An identifier, read as the `Ident` arm reads it.
    Local(Var<'p>),
    /// An int literal.
    Int(i64),
    /// A float literal.
    Float(f64),
    /// Any other literal's value, built at compile time.
    Const(Value),
    /// The value the preceding ops pushed (popped).
    Stack,
}

/// An int, float or bool result of the scalar fast path: a plain copy, so
/// computing, testing and dropping it costs no `Value` drop glue.
#[derive(Clone, Copy, Debug)]
pub(super) enum Scalar {
    Int(i64),
    Float(f64),
    Bool(bool),
}

impl Scalar {
    #[inline(always)]
    pub(super) fn value(self) -> Value {
        match self {
            Scalar::Int(n) => Value::Int(n),
            Scalar::Float(f) => Value::Float(f),
            Scalar::Bool(b) => Value::Bool(b),
        }
    }
}

/// Which condition an `if`/`while` branch tests (its panic text).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cond {
    If,
    While,
}

impl Cond {
    fn word(self) -> &'static str {
        match self {
            Cond::If => "if",
            Cond::While => "while",
        }
    }
}

/// One instruction. Every op runs against the activation's `Env` and operand
/// stack; "push"/"pop" below are operand-stack operations. Jump targets are op
/// indices.
pub(super) enum Op<'p> {
    /// Evaluate the node on the tree-walker and push its value. `eval` pops
    /// every scope it pushes on every outcome, so the scope count is the same
    /// after the op as before it.
    Tree(&'p Expr),
    /// `env.push()`, where `eval` pushes a scope (block, loop iteration).
    ScopePush,
    /// `env.pop()` of the innermost scope this activation pushed.
    ScopePop,
    /// Push a literal's value (built at compile time).
    Const(Value),
    /// Push an identifier's value: `get_var`, then `globals`, then a fn as a
    /// value, then the undefined-identifier panic.
    Load(Var<'p>),
    /// Pop and discard (a statement's unused value).
    Drop,
    /// Pop into an untyped `let`/`own`/`ref`: `define_var`.
    Define(Var<'p>),
    /// Pop into a typed `let`/`own`/`ref`: `bind_let` (refinement check,
    /// sized-int coercion, define).
    Let {
        var: Var<'p>,
        ty: &'p AxonType,
    },
    /// `x = value` in an AX-31 shape: `assign_in_place` first; when it did
    /// the assignment, jump to `done`, past the value and its store. It can
    /// only act when `x` holds a str or array, so it is not called otherwise.
    AssignInPlace {
        var: Var<'p>,
        value: &'p Expr,
        done: u32,
    },
    /// Pop into `x = value`: `assign_var`, else the undefined-variable panic.
    Store(Var<'p>),
    /// `x = l op r` in one op ([`Op::Bin`], then [`Op::Store`]). `in_place`
    /// is the value node when the statement has an AX-31 shape and both
    /// operands are inline: `assign_in_place` runs first, as
    /// [`Op::AssignInPlace`] does, unless the int/float fast path took the
    /// operands (then `x`, the left one, holds no str or array, and
    /// `assign_in_place` would have done nothing). With a stack operand, a
    /// separate `AssignInPlace` op precedes the operands instead.
    StoreBin {
        var: Var<'p>,
        in_place: Option<&'p Expr>,
        op: BinOp,
        l: Opnd<'p>,
        r: Opnd<'p>,
    },
    /// [`Op::StoreBin`] with a local left operand and an int literal right
    /// one (`i = i + 1`), so the int fast path decodes no operand kinds.
    StoreLocalInt {
        var: Var<'p>,
        in_place: Option<&'p Expr>,
        op: BinOp,
        l: Var<'p>,
        r: i64,
    },
    /// [`Op::StoreBin`] with two local operands (`s = s + i`).
    StoreLocalLocal {
        var: Var<'p>,
        in_place: Option<&'p Expr>,
        op: BinOp,
        l: Var<'p>,
        r: Var<'p>,
    },
    /// [`Op::StoreBin`] with a local left operand and the right one on the
    /// stack (`s = s + f(i)`, S5).
    StoreLocalStack {
        var: Var<'p>,
        op: BinOp,
        l: Var<'p>,
    },
    /// Push `l op r` (any operator but `&&`/`||`).
    Bin {
        op: BinOp,
        l: Opnd<'p>,
        r: Opnd<'p>,
    },
    /// `&&`/`||` after its left operand: when that decides the result, leave
    /// it as the value and jump to `end`.
    ShortCircuit {
        op: &'p BinOp,
        end: u32,
    },
    /// `&&`/`||` after its right operand: pop both, push the result.
    Logic(&'p BinOp),
    /// Pop, apply a unary operator other than `&mut`, push.
    Unary(&'p UnaryOp),
    Jump(u32),
    /// `env.pop()`, then jump: the end of a `while` iteration.
    PopJump(u32),
    /// Pop an `if`/`while` condition (`cond_bool`); jump to `target` when
    /// false. Otherwise, when `push` (a `while`), `env.push()` for the
    /// iteration.
    BranchFalse {
        target: u32,
        cond: Cond,
        push: bool,
    },
    /// A fused compare-and-branch: `l op r` as [`Op::Bin`] computes it, then
    /// as [`Op::BranchFalse`] (`cond_bool` whenever the result is not a
    /// plain bool).
    BranchCmp {
        op: BinOp,
        l: Opnd<'p>,
        r: Opnd<'p>,
        target: u32,
        cond: Cond,
        push: bool,
    },
    /// [`Op::BranchCmp`] with a local left operand and an int literal right
    /// one (`while i < 1000000`).
    BranchLocalInt {
        op: BinOp,
        l: Var<'p>,
        r: i64,
        target: u32,
        cond: Cond,
        push: bool,
    },
    /// [`Op::BranchCmp`] with two local operands (`while i < n`).
    BranchLocalLocal {
        op: BinOp,
        l: Var<'p>,
        r: Var<'p>,
        target: u32,
        cond: Cond,
        push: bool,
    },
    /// The value on top must be an `Int` (`strict_int`, a `for` bound).
    StrictInt,
    /// `for` head over the counter and bound on top of the stack: past the
    /// bound, jump to `exit`; else, as the `For` arm and `run_loop_body` do,
    /// `env.push()`, define the variable, `env.push()` (the body's scope,
    /// only when `scoped`: a body that binds nothing leaves it empty).
    ForTest {
        var: Var<'p>,
        inclusive: bool,
        scoped: bool,
        exit: u32,
    },
    /// The end of a `for` iteration: `env.pop()` of the body's scope (when
    /// `scoped`) and of the variable's, counter `+= 1`, then the test and
    /// pushes of [`Op::ForTest`]; past the bound it falls through to the
    /// exit, else it jumps to `first`.
    ForNext {
        var: Var<'p>,
        inclusive: bool,
        scoped: bool,
        first: u32,
    },
    /// Pop the body's result and end the body.
    Return,
    Break,
    Continue,
    /// Pop and apply `?`.
    Question,
    WrapSome,
    WrapOk,
    WrapErr,
    /// Push an empty string for an interpolation.
    FmtNew,
    /// Append literal text to the interpolation on top.
    FmtLit(&'p String),
    /// Pop a value and append its display to the interpolation below it.
    FmtPush,
    /// A call by name: pop `argc` arguments (pushed left to right) into a
    /// pooled argument buffer and dispatch them as `dispatch_call` does, the
    /// callee resolved at compile time (`dispatch_named`; `resume`, the one
    /// name `dispatch_call` handles before resolving, goes to
    /// `dispatch_resume`). `slow`: the flag that keeps this fn-table callee
    /// off [`Op::CallFast`] (S5), for the `vm: slow` trace line.
    Call {
        callee: Var<'p>,
        resume: bool,
        argc: u32,
        tier: Option<&'p str>,
        slow: Option<&'static str>,
    },
    // ── S2: aggregates, index and field reads, place writes ──
    /// Pop `n` values (pushed left to right), push them as an array.
    MakeArray(u32),
    /// Pop `n` values (pushed left to right), push them as a tuple.
    MakeTuple(u32),
    /// A non-empty struct or enum literal (the `StructLit` node) after its
    /// field values (pushed in source order): pop them into `finish_record`.
    /// A literal whose resolution is `empty` compiles to [`Op::Const`].
    Record(&'p Expr),
    /// `name.field` on an identifier: `field_in_place`, push.
    FieldLocal {
        var: Var<'p>,
        f: Sym,
        field: &'p String,
    },
    /// Pop a receiver, push its `.field` (`field_of`).
    Field {
        f: Sym,
        field: &'p String,
    },
    /// `name[i]` with an inline index (an identifier or a literal), as the
    /// `Index` arm runs it: when `name` is bound locally or in `globals`, the
    /// index (`strict_int`), then `index_in_place`; otherwise the `Ident`
    /// chain's value for `name`, then the index, then `index_value`.
    IndexLocal {
        arr: Var<'p>,
        idx: Opnd<'p>,
    },
    /// The head of `name[<index>]` with an index that needs ops of its own:
    /// push a unit when `name` is bound locally or in `globals` (read in
    /// place by [`Op::IndexIdent`]), else the `Ident` chain's value for it.
    IndexTest(Var<'p>),
    /// Pop the index (`strict_int`) and what [`Op::IndexTest`] pushed; push
    /// `index_in_place` for a unit, else `index_value` of that value.
    IndexIdent(Var<'p>),
    /// Pop the index (`strict_int`) and the receiver; push `index_value`.
    IndexValue,
    /// Pop an index of a place being written, push it through `place_index`.
    PlaceIndex,
    /// A place write `base.. = v`: pop the indices [`Op::PlaceIndex`] pushed
    /// (the base-most on top) and the value below them, then `write_place`
    /// along `steps` (base to leaf; an `Index` step's number is a
    /// placeholder filled from the stack). `nidx` counts the `Index` steps.
    WritePlace {
        base: Var<'p>,
        steps: Box<[PlaceStep]>,
        nidx: u32,
    },
    /// `base[i] = v` with an inline index: the value (popped, or read from
    /// `val` when it is an inline operand), then the index (`place_index`),
    /// then `write_place` with that one step.
    WriteIndexLocal {
        base: Var<'p>,
        idx: Opnd<'p>,
        val: Opnd<'p>,
    },
    /// A place whose root is not an identifier, after its value and index
    /// expressions: the `invalid assignment target` panic.
    PlaceInvalid,
    /// [`Op::BranchCmp`] on `name[i] op r` with `i` and `r` locals (`if a[j]
    /// < pivot`): the read as [`Op::IndexLocal`] does it, then the compare.
    /// An int element against an int compares without building a `Value`;
    /// anything else pushes the element and takes `BranchCmp`'s path with a
    /// stack left operand.
    BranchIndexLocal {
        op: BinOp,
        arr: Var<'p>,
        idx: Var<'p>,
        r: Var<'p>,
        target: u32,
        cond: Cond,
        push: bool,
    },
    /// [`Op::BranchIndexLocal`] with an int literal right operand (`if a[j]
    /// == 0`).
    BranchIndexInt {
        op: BinOp,
        arr: Var<'p>,
        idx: Var<'p>,
        r: i64,
        target: u32,
        cond: Cond,
        push: bool,
    },

    // ── S3: match, while let, method calls ──
    /// A `match` arm's head, the subject on top of the stack (it stays
    /// there for the later arms): `env.push()` for the arm (when `scoped`),
    /// then `match_pattern` binds into it; on no match, `env.pop()` and jump
    /// to `next`, the next arm. A pattern error propagates with the arm's
    /// scope counted, so it is popped as the `Match` arm pops it.
    MatchArm {
        pat: &'p Pattern,
        scoped: bool,
        next: u32,
    },
    /// Pop a match guard's value. Only a plain `true` takes the arm (no
    /// unwrap, no panic: an `Uncertain<bool>` is false); otherwise
    /// `env.pop()` of the arm's scope (when `scoped`) and jump to `next`.
    Guard {
        scoped: bool,
        next: u32,
    },
    /// The end of a taken arm: `env.pop()` (when `scoped`), drop the subject
    /// below the arm's value, jump to `end`.
    MatchEnd {
        scoped: bool,
        end: u32,
    },
    /// No arm matched: the `no match arm matched` panic.
    NoMatch,
    /// A `while let` head after its value: pop it, `env.push()` (when
    /// `scoped`), `match_pattern`; on no match `env.pop()` and jump to
    /// `exit`, else `env.push()` for the body as `run_loop_body` does (when
    /// `body`).
    WhileLet {
        pat: &'p Pattern,
        scoped: bool,
        body: bool,
        exit: u32,
    },
    /// The end of a `while let` iteration: `env.pop()` of the body's scope
    /// and then of the pattern's (each when pushed), jump to `head`.
    WhileLetNext {
        scoped: bool,
        body: bool,
        head: u32,
    },
    /// A method call after its receiver: a channel receiver is popped and
    /// `chan_method` runs with the argument nodes (evaluating them as the
    /// tree does), its value pushed, and control jumps to `done`, past the
    /// arguments and [`Op::MethodCall`]. Any other receiver stays, and the
    /// compiled arguments follow.
    MethodRecv {
        method: &'p str,
        args: &'p [Expr],
        done: u32,
    },
    /// Pop the receiver and `argc` arguments (pushed in that order) and call
    /// `impl_method` with them in place on the stack.
    MethodCall {
        method: &'p str,
        argc: u32,
    },

    // ── S4: lambdas ──
    /// Push the closure a lambda node makes in this env (`make_closure`,
    /// S4).
    Lambda(&'p Expr),

    // ── S7: pure scalar regions (spec §4 S7) ──
    /// Deliver a pure tree's value to its sink and continue at `skip`, or
    /// decline into the twin that follows ([`PureOp::run`]).
    Pure(Box<PureOp<'p>>),
    /// Run a `while` in registers and continue at `exit` when its condition
    /// went false, or decline into the generic loop that follows
    /// ([`PureLoop::run`]).
    PureLoop {
        lp: Box<PureLoop<'p>>,
        exit: u32,
    },
    // -- S5: call costs --
    /// A fast VM→VM call (spec §4 S5) to `fn_table[entry]`: `call_fast`,
    /// once `proven`. The arguments are `args` when it is not empty: inline
    /// operands (every argument an identifier or a literal, as many as the
    /// fn has parameters), read when the op runs as the `Ident`/`Literal`
    /// arms read them, and nothing is on the stack. Otherwise they are the
    /// top `argc` of the stack, as for [`Op::Call`]. The compiler has proven
    /// everything `dispatch_named` decides but whether `call_builtin` claims
    /// the name; its `callees` cache records that (`CALLEE_FN + entry`:
    /// proven not a builtin) after the first call by the name, which until
    /// then this op makes through `dispatch_named`.
    CallFast {
        callee: Var<'p>,
        entry: u32,
        argc: u32,
        args: Box<[Opnd<'p>]>,
        proven: Cell<bool>,
    },
    /// `f(.., &mut a, ..)` (S5): a call with one operand per argument in
    /// `args`, each plain one either inline (every plain argument an
    /// identifier or a literal, read here in order) or on the stack
    /// ([`Opnd::Stack`], `stacked` of them, pushed left to right), and a
    /// `Unit` constant in each `&mut` position; then each borrowed local of
    /// `refs` (in argument order) moved in. `fn_table[entry]` runs in a
    /// pooled frame and the params' final values move back:
    /// `call_mut_with`'s steps with the borrowed bindings resolved at
    /// compile time. With no entry (the callee is not a fn, or a `&mut`
    /// operand is not an identifier) `args` is empty and `call_mut` runs the
    /// whole call, panic included. A statement call (`discard`) drops its
    /// value instead of pushing it.
    CallMut {
        call: &'p Expr,
        entry: Option<u32>,
        args: Box<[Opnd<'p>]>,
        stacked: u32,
        refs: Box<[MutRef<'p>]>,
        discard: bool,
    },
}

/// One `&mut name` argument of an [`Op::CallMut`].
pub(super) struct MutRef<'p> {
    /// Its argument position.
    pub(super) arg: u32,
    /// The borrowed local.
    pub(super) var: Var<'p>,
    /// The callee's parameter at `arg` is `&mut`, so its final value moves
    /// back (else the binding gets `Unit`, as `call_mut_with` gives it).
    pub(super) back: bool,
}

/// The operand stack is malformed: the compiler pushed fewer values than an
/// op pops. A host bug (spec §6), never a user-reachable state.
#[cold]
#[inline(never)]
fn malformed() -> ! {
    unreachable!("vm: malformed op stream (operand stack underflow)")
}

#[inline(always)]
fn pop(st: &mut Vec<Value>) -> Value {
    match st.pop() {
        Some(v) => v,
        None => malformed(),
    }
}

/// `x = v` to a binding [`Env::get_var`] reads, as the `Assign` arm does
/// (`assign_var`). An int over an int is written in place: dropping the old
/// int is a no-op, so this skips only the drop glue.
#[inline(always)]
fn store(env: &mut Env, var: &Var<'_>, v: Value) -> Result<(), Flow> {
    match env.get_var_mut(var.s, var.slot) {
        Some(b) => {
            match (b, v) {
                (Value::Int(d), Value::Int(n)) => *d = n,
                (b, v) => *b = v,
            }
            Ok(())
        }
        None => panic(format!("assignment to undefined variable `{}`", var.name)),
    }
}

/// The string an interpolation builds, on top of the stack ([`Op::FmtNew`]).
fn fmt_top(st: &mut [Value]) -> &mut String {
    match st.last_mut() {
        Some(Value::Str(s)) => Rc::make_mut(s),
        _ => malformed(),
    }
}

/// Whether `x` holds a str or an array. `assign_in_place` acts on nothing
/// else and checks that before it reads or evaluates anything, so the engine
/// skips the call otherwise.
#[inline(always)]
fn appendable(env: &Env, var: &Var<'_>) -> bool {
    matches!(
        env.get_var(var.s, var.slot),
        Some(Value::Str(_) | Value::Array(_))
    )
}

/// `env.pop()` of the innermost scope this activation pushed.
#[inline(always)]
fn pop_scope(env: &mut Env, scopes: &mut u32) {
    let Some(n) = scopes.checked_sub(1) else {
        malformed()
    };
    env.pop();
    *scopes = n;
}

/// A `for` loop's counter and bound, the two values on top of the stack.
#[inline(always)]
fn for_bounds(st: &[Value]) -> (i64, i64) {
    match st {
        [.., Value::Int(i), Value::Int(e)] => (*i, *e),
        _ => malformed(),
    }
}

/// Enter a `for` iteration as the `For` arm does: `env.push()`, the loop
/// variable, then `run_loop_body`'s `env.push()` when the body is `scoped`.
#[inline(always)]
fn for_enter(env: &mut Env, scopes: &mut u32, var: &Var<'_>, i: i64, scoped: bool) {
    env.push();
    env.define_var(var.s, var.slot, Value::Int(i));
    *scopes += 1;
    if scoped {
        env.push();
        *scopes += 1;
    }
}

/// The next `for` iteration's `env.pop()`, `env.push()` and loop-variable
/// define in one write, when together they would change nothing but the
/// variable's value: the variable's scope holds only the variable, an int,
/// at the slot `define_var` would bind it to. `false` (nothing done) else.
#[inline(always)]
fn for_rebind(env: &mut Env, var: &Var<'_>, i: i64) -> bool {
    let Some(&m) = env.marks.last() else {
        return false;
    };
    if env.vars.len() != m + 1 || var.slot as usize != m {
        return false;
    }
    match env.vars.last_mut() {
        Some((s, Value::Int(n))) if *s == var.s => {
            *n = i;
            true
        }
        _ => false,
    }
}

/// `l op r` without building an [`Operand`]: when both operands are ints and
/// [`int_fast`] has the value, or both are floats and [`float_fast`] has it.
/// The operands are read in place and a stack operand is popped only then.
/// `None` in every other case, every panic included: nothing was read out or
/// popped, and the caller takes the general path (`operands`, then
/// `binop_operands`), which produces the value or the panic `eval_binop`
/// would. Never called for `&&`/`||`.
#[inline(always)]
fn scalar_fast(
    op: &BinOp,
    l: &Opnd<'_>,
    r: &Opnd<'_>,
    env: &Env,
    st: &mut Vec<Value>,
) -> Option<Scalar> {
    let n = st.len();
    let r_stacked = matches!(r, Opnd::Stack) as usize;
    let stacked = r_stacked + matches!(l, Opnd::Stack) as usize;
    let v = match (
        scalar_at(l, env, st, n.wrapping_sub(1 + r_stacked))?,
        scalar_at(r, env, st, n.wrapping_sub(1))?,
    ) {
        (Scalar::Int(a), Scalar::Int(b)) => int_fast(op, a, b)?,
        (Scalar::Float(a), Scalar::Float(b)) => float_fast(op, a, b)?,
        _ => return None,
    };
    for _ in 0..stacked {
        // An int or a float: nothing to drop.
        std::mem::forget(st.pop());
    }
    Some(v)
}

/// The int or float operand `o` reads; `at` is its stack index when it is
/// on the stack. `None` for any other value and for an unbound identifier.
#[inline(always)]
fn scalar_at(o: &Opnd<'_>, env: &Env, st: &[Value], at: usize) -> Option<Scalar> {
    let v = match o {
        Opnd::Int(n) => return Some(Scalar::Int(*n)),
        Opnd::Float(f) => return Some(Scalar::Float(*f)),
        Opnd::Local(var) => env.get_var(var.s, var.slot)?,
        Opnd::Stack => st.get(at)?,
        Opnd::Const(_) => return None,
    };
    match v {
        Value::Int(n) => Some(Scalar::Int(*n)),
        Value::Float(f) => Some(Scalar::Float(*f)),
        _ => None,
    }
}

/// `a op b` on `i64`s when [`int_binop`] yields `Ok`: the same value. `None`
/// when it panics (overflow, division or remainder by zero) or has no arm
/// (`&&`/`||`). The int half of [`scalar_fast`]; the unit test
/// `int_fast_agrees_with_int_binop` pins it to `int_binop`.
#[inline(always)]
pub(super) fn int_fast(op: &BinOp, a: i64, b: i64) -> Option<Scalar> {
    use BinOp::*;
    use Scalar::{Bool, Int};
    Some(match op {
        Add => Int(a.checked_add(b)?),
        Sub => Int(a.checked_sub(b)?),
        Mul => Int(a.checked_mul(b)?),
        Div => Int(a.checked_div(b)?),
        Rem if b == 0 => return None,
        Rem => Int(a.wrapping_rem(b)),
        Eq => Bool(a == b),
        NotEq => Bool(a != b),
        Lt => Bool(a < b),
        Gt => Bool(a > b),
        LtEq => Bool(a <= b),
        GtEq => Bool(a >= b),
        BitAnd => Int(a & b),
        BitOr => Int(a | b),
        BitXor => Int(a ^ b),
        Shl => Int(a.wrapping_shl(b as u32)),
        Shr => Int(a.wrapping_shr(b as u32)),
        And | Or => return None,
    })
}

/// `a op b` on `f64`s when [`float_binop`] has an arm: the same value. The
/// float half of [`scalar_fast`]; `float_fast_agrees_with_float_binop` pins
/// it to `float_binop`.
#[inline(always)]
pub(super) fn float_fast(op: &BinOp, a: f64, b: f64) -> Option<Scalar> {
    use BinOp::*;
    use Scalar::{Bool, Float};
    Some(match op {
        Add => Float(a + b),
        Sub => Float(a - b),
        Mul => Float(a * b),
        Div => Float(a / b),
        Rem => Float(a % b),
        Eq => Bool(a == b),
        NotEq => Bool(a != b),
        Lt => Bool(a < b),
        Gt => Bool(a > b),
        LtEq => Bool(a <= b),
        GtEq => Bool(a >= b),
        And | Or | BitAnd | BitOr | BitXor | Shl | Shr => return None,
    })
}

/// `x = s` for a scalar fast-path result, as [`store`] does: an int over an
/// int or a float over a float is written in place.
#[inline(always)]
fn store_scalar(env: &mut Env, var: &Var<'_>, s: Scalar) -> Result<(), Flow> {
    match env.get_var_mut(var.s, var.slot) {
        Some(b) => {
            match (b, s) {
                (Value::Int(d), Scalar::Int(n)) => *d = n,
                (Value::Float(d), Scalar::Float(f)) => *d = f,
                (b, s) => *b = s.value(),
            }
            Ok(())
        }
        None => panic(format!("assignment to undefined variable `{}`", var.name)),
    }
}

/// The int a local holds; `None` for any other value or an unbound name.
#[inline(always)]
fn local_int(env: &Env, var: &Var<'_>) -> Option<i64> {
    match env.get_var(var.s, var.slot)? {
        Value::Int(n) => Some(*n),
        _ => None,
    }
}

/// [`scalar_fast`] over two locals.
#[inline(always)]
fn locals_fast(op: &BinOp, l: &Var<'_>, r: &Var<'_>, env: &Env) -> Option<Scalar> {
    match (env.get_var(l.s, l.slot)?, env.get_var(r.s, r.slot)?) {
        (Value::Int(a), Value::Int(b)) => int_fast(op, *a, *b),
        (Value::Float(a), Value::Float(b)) => float_fast(op, *a, *b),
        _ => None,
    }
}

/// The stack index of the first of the top `n` values.
#[inline(always)]
fn tail(st: &[Value], n: u32) -> usize {
    match st.len().checked_sub(n as usize) {
        Some(at) => at,
        None => malformed(),
    }
}

/// The int `arr[idx]` holds when the local `arr` holds an array, the local
/// `idx` an int in its bounds, and the element is an int: what
/// [`index_fast`] would read. `None` in every other case (nothing read out).
#[inline(always)]
fn index_int(env: &Env, arr: &Var<'_>, idx: &Var<'_>) -> Option<i64> {
    let Value::Array(items) = env.get_var(arr.s, arr.slot)? else {
        return None;
    };
    match items.get(local_int(env, idx)? as usize)? {
        Value::Int(n) => Some(*n),
        _ => None,
    }
}

/// [`Op::IndexLocal`] when the local holds an array and the index is an int
/// literal or a local holding an int that is in bounds: the element, as
/// `index_in_place` reads it. `None` in every other case (nothing was read
/// out); the slow path then runs the arm's whole protocol.
#[inline(always)]
fn index_fast(env: &Env, arr: &Var<'_>, idx: &Opnd<'_>) -> Option<Value> {
    let Value::Array(items) = env.get_var(arr.s, arr.slot)? else {
        return None;
    };
    let i = match idx {
        Opnd::Int(n) => *n,
        Opnd::Local(v) => match env.get_var(v.s, v.slot)? {
            Value::Int(n) => *n,
            _ => return None,
        },
        _ => return None,
    };
    Some(match items.get(i as usize)? {
        Value::Int(n) => Value::Int(*n),
        Value::Float(f) => Value::Float(*f),
        v => v.clone(),
    })
}

/// [`Op::WriteIndexLocal`] when the local holds a uniquely owned array and
/// `i` is in its bounds: what `write_place` does there (`make_mut` copies
/// nothing), the element set in place without the step walk (cost only,
/// S5). `Some(v)` hands the value back for `write_place` in every other
/// case, nothing written.
#[inline(always)]
fn write_index_unique(env: &mut Env, base: &Var<'_>, i: usize, v: Value) -> Option<Value> {
    let Some(Value::Array(items)) = env.get_var_mut(base.s, base.slot) else {
        return Some(v);
    };
    match Rc::get_mut(items).and_then(|a| a.get_mut(i)) {
        Some(slot) => {
            forget_scalar(std::mem::replace(slot, v));
            None
        }
        None => Some(v),
    }
}

/// [`Op::WritePlace`]: the steps with the indices on the stack filled in
/// (the base-most index on top), then `write_place` with the value below
/// them.
#[inline(never)]
fn write_place_op(
    base: &Var<'_>,
    steps: &[PlaceStep],
    nidx: u32,
    env: &mut Env,
    st: &mut Vec<Value>,
) -> Result<(), Flow> {
    let at = tail(st, nidx + 1);
    let mut top = st.len();
    let mut fill = |s: &PlaceStep| match s {
        PlaceStep::Index(_) => {
            top -= 1;
            match st[top] {
                Value::Int(i) => PlaceStep::Index(i as usize),
                _ => malformed(),
            }
        }
        f => *f,
    };
    let mut small = [PlaceStep::Index(0); 8];
    let large: Vec<PlaceStep>;
    let filled: &[PlaceStep] = if steps.len() <= small.len() {
        for (d, s) in small.iter_mut().zip(steps) {
            *d = fill(s);
        }
        &small[..steps.len()]
    } else {
        large = steps.iter().map(fill).collect();
        &large
    };
    let v = std::mem::replace(&mut st[at], Value::Unit);
    st.truncate(at);
    write_place(base.s, base.slot, filled, v, env)
}

impl<'p> Interp<'p> {
    /// Run a fn body: compiled under [`Engine::Vm`] for a fn-table entry, on
    /// the tree otherwise. `call_fn_in` is the only caller, at the point where
    /// it would evaluate `entry.def.body`; everything before and after the
    /// body stays there (spec §4 Activation).
    #[inline]
    pub(super) fn run_body(&self, entry: &FnEntry<'_>, env: &mut Env) -> R {
        match self.engine {
            Engine::Tree => self.eval(&entry.def.body, env),
            Engine::Vm => self.run_body_vm(entry, env),
        }
    }

    #[inline(never)]
    fn run_body_vm(&self, entry: &FnEntry<'_>, env: &mut Env) -> R {
        let Some(cell) = &entry.compiled else {
            self.vm_trace_tree(entry, "not in fn table");
            return self.eval(&entry.def.body, env);
        };
        let body = cell.get_or_init(|| {
            let body = compile(self, &entry.def.body);
            if self.vm_trace {
                self.vm_trace_compiled(entry, &body);
            }
            body
        });
        self.exec(body, env)
    }

    /// Run a lambda body (S4): compiled under [`Engine::Vm`] for a code the
    /// resolution table built, on the tree otherwise. `call_closure_owned_by`
    /// is the only caller, with the env it built (captures, then the
    /// parameters' scope); everything before and after the body stays there.
    /// A `Flow::Return(v)` out of the body is its value `Ok(v)`.
    #[inline(always)]
    pub(super) fn run_lambda(&self, code: &ClosureCode, env: &mut Env) -> R {
        if self.engine == Engine::Vm {
            if let Some(body) = code.compiled.as_ref().and_then(|lb| lb.compiled()) {
                if let Some(v) = body.leaf_value(env, false) {
                    return Ok(v);
                }
                return self.exec(body, env);
            }
        }
        self.run_lambda_cold(code, env)
    }

    /// [`Interp::run_lambda`] on the tree, or for a body's first run.
    #[inline(never)]
    fn run_lambda_cold(&self, code: &ClosureCode, env: &mut Env) -> R {
        let out = match (self.engine, &code.compiled) {
            (Engine::Tree, _) => self.eval(&code.body, env),
            (Engine::Vm, None) => {
                // No name and no compiled body: one line per code instance.
                if self.vm_trace && !code.traced.replace(true) {
                    eprintln!("vm: tree <anon>: unresolved lambda");
                }
                self.eval(&code.body, env)
            }
            (Engine::Vm, Some(lb)) => {
                let body = lb.get(code, |src| {
                    let body = compile(self, src);
                    if self.vm_trace {
                        self.vm_trace_named(&lb.name, &body);
                    }
                    body
                });
                return self.exec(body, env);
            }
        };
        match out {
            Err(Flow::Return(v)) => Ok(v),
            out => out,
        }
    }

    /// Execute `body` against `env` on an operand stack of its own (the
    /// frame's `Env::stack`, empty for a nested activation on the same frame).
    /// A `Flow::Break`/`Flow::Continue` out of an op in a
    /// loop's iteration, a callee or a `Tree` op included, is caught by the
    /// innermost such loop: its iteration's scopes are popped and its stack
    /// temporaries dropped. On every exit, normal or `Err`, the scopes this
    /// activation pushed are popped one at a time, innermost first (never
    /// truncated to a recorded depth: `call_mut` reads `&mut` params back by
    /// name afterwards, spec §4 Execution). `Flow::Return(v)` ends the body
    /// with `Ok(v)`, which `call_fn_in` treats exactly as the
    /// `Err(Flow::Return(v))` the tree-walker's body yields; every other
    /// `Flow` propagates unchanged. Inlined into each caller, so a fn call
    /// does not pay a call into it (cost only).
    #[inline(always)]
    pub(super) fn exec(&self, body: &Body<'_>, env: &mut Env) -> R {
        // A pooled frame's stack keeps the capacity it grew to; a new one
        // starts at the body's height so it does not regrow while the body
        // runs.
        let mut st = std::mem::take(&mut env.stack);
        if st.capacity() == 0 {
            st.reserve_exact(body.max_stack);
        }
        let mut scopes = 0u32;
        let mut pc = 0usize;
        let mut out = self.run(body, env, &mut st, &mut scopes, &mut pc);
        if matches!(out, Err(Flow::Break | Flow::Continue)) {
            out = self.exec_catching(body, env, &mut st, 0, &mut scopes, &mut pc, out);
        }
        for _ in 0..scopes {
            env.pop();
        }
        if st.capacity() <= 4096 {
            if !st.is_empty() {
                st.clear();
            }
            // The slot holds the empty `Vec` `take` left (nothing runs a
            // body on this frame meanwhile): forget it rather than call its
            // drop glue (cost only, S5).
            let old = std::mem::replace(&mut env.stack, st);
            if old.capacity() == 0 {
                std::mem::forget(old);
            }
        }
        match out {
            Err(Flow::Return(v)) => Ok(v),
            out => out,
        }
    }

    /// [`Interp::exec`] for a fast call (S5): the body runs on the caller's
    /// operand stack `st` above its current height, the callee's values
    /// stacked on the caller's as the tree-walker's Rust frames nest them,
    /// so the activation neither takes nor returns a stack of its own (cost
    /// only). Every op addresses the stack relative to its top; the loop
    /// heights a caught `Break`/`Continue` trims to are offset by the base.
    /// The base is restored on every exit (temporaries an `Err` left are
    /// dropped here, as `exec` drops them when it clears its stack).
    #[inline(always)]
    fn exec_on(&self, body: &Body<'_>, env: &mut Env, st: &mut Vec<Value>) -> R {
        let base = st.len();
        let mut scopes = 0u32;
        let mut pc = 0usize;
        let mut out = self.run(body, env, st, &mut scopes, &mut pc);
        if matches!(out, Err(Flow::Break | Flow::Continue)) {
            out = self.exec_catching(body, env, st, base, &mut scopes, &mut pc, out);
        }
        for _ in 0..scopes {
            env.pop();
        }
        if st.len() > base {
            st.truncate(base);
        }
        match out {
            Err(Flow::Return(v)) => Ok(v),
            out => out,
        }
    }

    /// [`Interp::exec`] after `run` stopped on a `Flow::Break`/`Continue`:
    /// the innermost loop of this body holding the failing op catches it
    /// (pops back to its scopes, trims the stack to `base` plus its height,
    /// resumes at its target); with no such loop the flow propagates.
    /// Repeats until `run` ends some other way.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn exec_catching(
        &self,
        body: &Body<'_>,
        env: &mut Env,
        st: &mut Vec<Value>,
        base: usize,
        scopes: &mut u32,
        pc: &mut usize,
        mut out: R,
    ) -> R {
        loop {
            match out {
                Err(flow @ (Flow::Break | Flow::Continue)) => {
                    // `pc` is one past the op that failed.
                    let Some(l) = body.loop_at(*pc as u32 - 1) else {
                        return Err(flow);
                    };
                    let (keep, to) = match flow {
                        Flow::Break => (l.scopes, l.brk),
                        _ => (l.cont_scopes, l.cont),
                    };
                    while *scopes > keep {
                        env.pop();
                        *scopes -= 1;
                    }
                    st.truncate(base + l.height as usize);
                    *pc = to as usize;
                    out = self.run(body, env, st, scopes, pc);
                }
                out => return out,
            }
        }
    }

    /// The op loop: runs from `*pc` until the body's value is ready (the ops
    /// ran out, or [`Op::Return`]) or an op errs. On `Err`, `*pc` is one past
    /// the failing op. On every exit `*scopes` counts the scopes still
    /// pushed (kept in a local while the loop runs).
    fn run(
        &self,
        body: &Body<'_>,
        env: &mut Env,
        st: &mut Vec<Value>,
        scopes_out: &mut u32,
        pc_out: &mut usize,
    ) -> R {
        let ops = &body.ops[..];
        let mut pc = *pc_out;
        let mut scope_count = *scopes_out;
        let scopes = &mut scope_count;
        macro_rules! tri {
            ($e:expr) => {
                match $e {
                    Ok(v) => v,
                    Err(flow) => {
                        *pc_out = pc;
                        *scopes_out = *scopes;
                        return Err(flow);
                    }
                }
            };
        }
        loop {
            let Some(op) = ops.get(pc) else {
                *scopes_out = *scopes;
                // Not `unwrap_or(Value::Unit)`: that builds and drops a
                // `Unit` (a drop-glue call) on every body exit.
                return Ok(match st.pop() {
                    Some(v) => v,
                    None => Value::Unit,
                });
            };
            pc += 1;
            match op {
                Op::StoreBin {
                    var,
                    in_place,
                    op,
                    l,
                    r,
                } => match scalar_fast(op, l, r, env, st) {
                    Some(s) => tri!(store_scalar(env, var, s)),
                    None => tri!(self.store_bin_slow(var, *in_place, op, l, r, env, st)),
                },
                Op::StoreLocalInt {
                    var,
                    in_place,
                    op,
                    l,
                    r,
                } => match local_int(env, l).and_then(|a| int_fast(op, a, *r)) {
                    Some(s) => tri!(store_scalar(env, var, s)),
                    None => {
                        let (l, r) = (Opnd::Local(*l), Opnd::Int(*r));
                        tri!(self.store_bin_slow(var, *in_place, op, &l, &r, env, st))
                    }
                },
                Op::StoreLocalLocal {
                    var,
                    in_place,
                    op,
                    l,
                    r,
                } => match locals_fast(op, l, r, env) {
                    Some(s) => tri!(store_scalar(env, var, s)),
                    None => {
                        let (l, r) = (Opnd::Local(*l), Opnd::Local(*r));
                        tri!(self.store_bin_slow(var, *in_place, op, &l, &r, env, st))
                    }
                },
                Op::StoreLocalStack { var, op, l } => {
                    let fast = match st.last() {
                        Some(Value::Int(b)) => local_int(env, l).and_then(|a| int_fast(op, a, *b)),
                        _ => None,
                    };
                    match fast {
                        Some(s) => {
                            // The popped operand is an int: nothing to drop.
                            std::mem::forget(st.pop());
                            tri!(store_scalar(env, var, s))
                        }
                        None => tri!(self.store_local_stack_slow(var, op, l, env, st)),
                    }
                }
                Op::BranchCmp {
                    op,
                    l,
                    r,
                    target,
                    cond,
                    push,
                } => {
                    let b = match scalar_fast(op, l, r, env, st) {
                        Some(Scalar::Bool(b)) => b,
                        Some(s) => tri!(cond_bool(s.value(), cond.word())),
                        None => tri!(self.cmp_slow(op, l, r, *cond, env, st)),
                    };
                    if !b {
                        pc = *target as usize;
                    } else if *push {
                        env.push();
                        *scopes += 1;
                    }
                }
                Op::BranchLocalInt {
                    op,
                    l,
                    r,
                    target,
                    cond,
                    push,
                } => {
                    let b = match local_int(env, l).and_then(|a| int_fast(op, a, *r)) {
                        Some(Scalar::Bool(b)) => b,
                        Some(s) => tri!(cond_bool(s.value(), cond.word())),
                        None => {
                            let (l, r) = (Opnd::Local(*l), Opnd::Int(*r));
                            tri!(self.cmp_slow(op, &l, &r, *cond, env, st))
                        }
                    };
                    if !b {
                        pc = *target as usize;
                    } else if *push {
                        env.push();
                        *scopes += 1;
                    }
                }
                Op::BranchLocalLocal {
                    op,
                    l,
                    r,
                    target,
                    cond,
                    push,
                } => {
                    let b = match locals_fast(op, l, r, env) {
                        Some(Scalar::Bool(b)) => b,
                        Some(s) => tri!(cond_bool(s.value(), cond.word())),
                        None => {
                            let (l, r) = (Opnd::Local(*l), Opnd::Local(*r));
                            tri!(self.cmp_slow(op, &l, &r, *cond, env, st))
                        }
                    };
                    if !b {
                        pc = *target as usize;
                    } else if *push {
                        env.push();
                        *scopes += 1;
                    }
                }
                Op::Bin { op, l, r } => {
                    let v = match scalar_fast(op, l, r, env, st) {
                        Some(s) => s.value(),
                        None => {
                            let (l, r) = tri!(self.operands(l, r, env, st));
                            tri!(binop_operands(op, l, r))
                        }
                    };
                    st.push(v);
                }
                Op::Load(var) => {
                    let v = match env.get_var(var.s, var.slot) {
                        Some(Value::Int(n)) => Value::Int(*n),
                        Some(Value::Float(f)) => Value::Float(*f),
                        Some(v) => v.clone(),
                        None => tri!(self.ident_unbound(var.name, var.s)),
                    };
                    st.push(v);
                }
                Op::Const(v) => st.push(match v {
                    Value::Int(n) => Value::Int(*n),
                    Value::Unit => Value::Unit,
                    v => v.clone(),
                }),
                Op::ScopePush => {
                    env.push();
                    *scopes += 1;
                }
                Op::ScopePop => pop_scope(env, scopes),
                Op::Jump(t) => pc = *t as usize,
                Op::PopJump(t) => {
                    pop_scope(env, scopes);
                    pc = *t as usize;
                }
                Op::Pure(p) => {
                    if let Some(to) = p.run(env, scopes) {
                        pc = to as usize;
                    }
                }
                Op::PureLoop { lp, exit } => {
                    if lp.run(env) {
                        pc = *exit as usize;
                    }
                }
                Op::BranchFalse { target, cond, push } => {
                    let b = match pop(st) {
                        Value::Bool(b) => b,
                        v => tri!(cond_bool(v, cond.word())),
                    };
                    if !b {
                        pc = *target as usize;
                    } else if *push {
                        env.push();
                        *scopes += 1;
                    }
                }
                Op::Call {
                    callee,
                    resume,
                    argc,
                    tier,
                    ..
                } => {
                    let Some(at) = st.len().checked_sub(*argc as usize) else {
                        malformed()
                    };
                    let v = if *resume {
                        let mut argv = self.take_args(*argc as usize);
                        argv.extend(st.drain(at..));
                        self.dispatch_resume(argv)
                    } else {
                        // The arguments stay on the stack: a call that binds
                        // them to a fn's parameters moves them from there.
                        let argv = StackTail { st: &mut *st, at };
                        self.dispatch_named(callee.name, callee.s, callee.slot, argv, *tier, env)
                    };
                    st.push(tri!(v));
                }
                Op::CallFast {
                    callee,
                    entry,
                    argc,
                    args,
                    proven,
                } => {
                    let v = if proven.get() || self.fast_call_proven(callee, *entry, proven) {
                        let entry = &self.fn_table[*entry as usize];
                        if args.is_empty() {
                            let Some(at) = st.len().checked_sub(*argc as usize) else {
                                malformed()
                            };
                            self.call_fast(entry, st, at)
                        } else {
                            self.call_fast_inline(entry, args, env, st)
                        }
                    } else {
                        self.call_unproven(callee, *argc, args, env, st)
                    };
                    st.push(tri!(v));
                }
                Op::CallMut {
                    call,
                    entry,
                    args,
                    stacked,
                    refs,
                    discard,
                } => {
                    let v = tri!(self.call_mut_op(call, *entry, args, *stacked, refs, env, st));
                    if *discard {
                        forget_scalar(v);
                    } else {
                        st.push(v);
                    }
                }
                Op::Store(var) => {
                    let v = pop(st);
                    tri!(store(env, var, v));
                }
                Op::Define(var) => {
                    let v = pop(st);
                    env.define_var(var.s, var.slot, v);
                }
                Op::Drop => drop(pop(st)),
                Op::ForTest {
                    var,
                    inclusive,
                    scoped,
                    exit,
                } => {
                    let (i, e) = for_bounds(st);
                    if if *inclusive { i <= e } else { i < e } {
                        for_enter(env, scopes, var, i, *scoped);
                    } else {
                        pc = *exit as usize;
                    }
                }
                Op::ForNext {
                    var,
                    inclusive,
                    scoped,
                    first,
                } => {
                    if *scoped {
                        pop_scope(env, scopes);
                    }
                    let [.., Value::Int(i), Value::Int(e)] = &mut st[..] else {
                        malformed()
                    };
                    *i += 1;
                    let (i, e) = (*i, *e);
                    let more = if *inclusive { i <= e } else { i < e };
                    if !more {
                        pop_scope(env, scopes);
                    } else if for_rebind(env, var, i) {
                        if *scoped {
                            env.push();
                            *scopes += 1;
                        }
                        pc = *first as usize;
                    } else {
                        pop_scope(env, scopes);
                        for_enter(env, scopes, var, i, *scoped);
                        pc = *first as usize;
                    }
                }
                Op::Return => {
                    *scopes_out = *scopes;
                    return Ok(pop(st));
                }
                Op::Tree(e) => st.push(tri!(self.eval(e, env))),
                Op::Let { var, ty } => {
                    let v = pop(st);
                    tri!(self.bind_let(var.name, var.s, var.slot, Some(ty), v, env));
                }
                Op::AssignInPlace { var, value, done } => {
                    if appendable(env, var)
                        && tri!(self.assign_in_place(var.s, var.slot, var.name, value, env))
                    {
                        pc = *done as usize;
                    }
                }
                Op::ShortCircuit { op, end } => {
                    if short_circuits(op, st.last().unwrap_or_else(|| malformed())) {
                        pc = *end as usize;
                    }
                }
                Op::Logic(op) => {
                    let rv = pop(st);
                    let lv = pop(st);
                    st.push(tri!(logic_rhs(op, lv, rv)));
                }
                Op::Unary(op) => {
                    let v = pop(st);
                    st.push(tri!(eval_unary(op, v)));
                }
                Op::StrictInt => {
                    let n = tri!(strict_int(pop(st)));
                    st.push(Value::Int(n));
                }
                Op::Break => tri!(Err(Flow::Break)),
                Op::Continue => tri!(Err(Flow::Continue)),
                Op::Question => {
                    let v = tri!(question(pop(st)));
                    st.push(v);
                }
                Op::WrapSome => {
                    let v = pop(st);
                    st.push(Value::Some(Box::new(v)));
                }
                Op::WrapOk => {
                    let v = pop(st);
                    st.push(Value::Ok(Box::new(v)));
                }
                Op::WrapErr => {
                    let v = pop(st);
                    st.push(Value::Err(Box::new(v)));
                }
                Op::FmtNew => st.push(Value::Str(Rc::new(String::new()))),
                Op::FmtLit(t) => fmt_top(st).push_str(t),
                Op::FmtPush => {
                    let v = pop(st);
                    fmt_top(st).push_str(&display(&v));
                }
                Op::MakeArray(n) => {
                    let at = tail(st, *n);
                    let items: Vec<Value> = st.drain(at..).collect();
                    st.push(Value::Array(Rc::new(items)));
                }
                Op::MakeTuple(n) => {
                    let at = tail(st, *n);
                    let items: Vec<Value> = st.drain(at..).collect();
                    st.push(Value::tuple(items));
                }
                Op::Record(e) => {
                    let v = tri!(self.record_op(e, st));
                    st.push(v);
                }
                Op::FieldLocal { var, f, field } => {
                    let v = tri!(self.field_in_place(var.s, var.slot, var.name, *f, field, env));
                    st.push(v);
                }
                Op::Field { f, field } => {
                    let r = pop(st);
                    st.push(tri!(field_of(&r, *f, field)));
                }
                Op::IndexLocal { arr, idx } => {
                    let v = match index_fast(env, arr, idx) {
                        Some(v) => v,
                        None => tri!(self.index_local_slow(arr, idx, env)),
                    };
                    st.push(v);
                }
                Op::IndexTest(var) => {
                    let v = if self.bound_in_place(var, env) {
                        Value::Unit
                    } else {
                        tri!(self.ident_unbound(var.name, var.s))
                    };
                    st.push(v);
                }
                Op::IndexIdent(var) => {
                    let i = pop(st);
                    let recv = pop(st);
                    let i = tri!(strict_int(i));
                    let v = match recv {
                        Value::Unit => self.index_in_place(var.s, var.slot, var.name, i, env),
                        recv => index_value(recv, i),
                    };
                    st.push(tri!(v));
                }
                Op::IndexValue => {
                    let i = pop(st);
                    let recv = pop(st);
                    let i = tri!(strict_int(i));
                    st.push(tri!(index_value(recv, i)));
                }
                Op::PlaceIndex => {
                    let v = pop(st);
                    let i = tri!(place_index(&v));
                    st.push(Value::Int(i as i64));
                }
                Op::WritePlace { base, steps, nidx } => {
                    tri!(write_place_op(base, steps, *nidx, env, st));
                }
                Op::WriteIndexLocal { base, idx, val } => {
                    let v = match val {
                        Opnd::Stack => pop(st),
                        Opnd::Int(n) => Value::Int(*n),
                        Opnd::Local(var) => match env.get_var(var.s, var.slot) {
                            Some(Value::Int(n)) => Value::Int(*n),
                            Some(v) => v.clone(),
                            None => tri!(self.ident_unbound(var.name, var.s)),
                        },
                        o => tri!(self.opnd_value(o, env)),
                    };
                    tri!(self.write_index_local(base, idx, v, env));
                }
                Op::PlaceInvalid => tri!(Err(Flow::Panic("invalid assignment target".into()))),
                Op::BranchIndexLocal {
                    op,
                    arr,
                    idx,
                    r,
                    target,
                    cond,
                    push,
                } => {
                    let fast =
                        index_int(env, arr, idx).and_then(|a| int_fast(op, a, local_int(env, r)?));
                    let b = match fast {
                        Some(Scalar::Bool(b)) => b,
                        Some(s) => tri!(cond_bool(s.value(), cond.word())),
                        None => {
                            let (idx, r) = (Opnd::Local(*idx), Opnd::Local(*r));
                            tri!(self.branch_index_slow(op, arr, &idx, &r, *cond, env, st))
                        }
                    };
                    if !b {
                        pc = *target as usize;
                    } else if *push {
                        env.push();
                        *scopes += 1;
                    }
                }
                Op::BranchIndexInt {
                    op,
                    arr,
                    idx,
                    r,
                    target,
                    cond,
                    push,
                } => {
                    let b = match index_int(env, arr, idx).and_then(|a| int_fast(op, a, *r)) {
                        Some(Scalar::Bool(b)) => b,
                        Some(s) => tri!(cond_bool(s.value(), cond.word())),
                        None => {
                            let (idx, r) = (Opnd::Local(*idx), Opnd::Int(*r));
                            tri!(self.branch_index_slow(op, arr, &idx, &r, *cond, env, st))
                        }
                    };
                    if !b {
                        pc = *target as usize;
                    } else if *push {
                        env.push();
                        *scopes += 1;
                    }
                }
                Op::MatchArm { pat, scoped, next } => {
                    if *scoped {
                        env.push();
                        *scopes += 1;
                    }
                    let Some(v) = st.last() else { malformed() };
                    if !tri!(self.match_pattern(pat, v, env)) {
                        if *scoped {
                            pop_scope(env, scopes);
                        }
                        pc = *next as usize;
                    }
                }
                Op::Guard { scoped, next } => {
                    if !matches!(pop(st), Value::Bool(true)) {
                        if *scoped {
                            pop_scope(env, scopes);
                        }
                        pc = *next as usize;
                    }
                }
                Op::MatchEnd { scoped, end } => {
                    if *scoped {
                        pop_scope(env, scopes);
                    }
                    let Some(at) = st.len().checked_sub(2) else {
                        malformed()
                    };
                    drop(st.swap_remove(at));
                    pc = *end as usize;
                }
                Op::NoMatch => {
                    tri!(panic("no match arm matched"));
                }
                Op::WhileLet {
                    pat,
                    scoped,
                    body,
                    exit,
                } => {
                    let v = pop(st);
                    if *scoped {
                        env.push();
                        *scopes += 1;
                    }
                    if tri!(self.match_pattern(pat, &v, env)) {
                        if *body {
                            env.push();
                            *scopes += 1;
                        }
                    } else {
                        if *scoped {
                            pop_scope(env, scopes);
                        }
                        pc = *exit as usize;
                    }
                }
                Op::WhileLetNext { scoped, body, head } => {
                    if *body {
                        pop_scope(env, scopes);
                    }
                    if *scoped {
                        pop_scope(env, scopes);
                    }
                    pc = *head as usize;
                }
                Op::MethodRecv { method, args, done } => {
                    if let Some(Value::Chan(_)) = st.last() {
                        let Value::Chan(q) = pop(st) else { malformed() };
                        st.push(tri!(self.chan_method(&q, method, args, env)));
                        pc = *done as usize;
                    }
                }
                Op::MethodCall { method, argc } => {
                    let Some(at) = st.len().checked_sub(*argc as usize + 1) else {
                        malformed()
                    };
                    // The receiver and arguments stay on the stack: the
                    // callee's binder moves them from there.
                    let v = self.impl_method(method, StackTail { st: &mut *st, at });
                    st.push(tri!(v));
                }
                Op::Lambda(e) => st.push(self.make_closure(e, env)),
            }
        }
    }

    /// [`Op::StoreLocalStack`] when its int fast path declined: the scalar
    /// fast path over the local and the stack operand, else
    /// [`Interp::store_bin_slow`]. Out of line so the op loop stays small.
    #[inline(never)]
    fn store_local_stack_slow(
        &self,
        var: &Var<'_>,
        op: &BinOp,
        l: &Var<'_>,
        env: &mut Env,
        st: &mut Vec<Value>,
    ) -> Result<(), Flow> {
        let l = Opnd::Local(*l);
        match scalar_fast(op, &l, &Opnd::Stack, env, st) {
            Some(s) => store_scalar(env, var, s),
            None => self.store_bin_slow(var, None, op, &l, &Opnd::Stack, env, st),
        }
    }

    /// `x = l op r` when the scalar fast path declined: `assign_in_place`
    /// first for an AX-31 shape (the fast path declining leaves that order
    /// intact: it read nothing out), then the general operands and
    /// `binop_operands`, then the store.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn store_bin_slow(
        &self,
        var: &Var<'_>,
        in_place: Option<&Expr>,
        op: &BinOp,
        l: &Opnd<'_>,
        r: &Opnd<'_>,
        env: &mut Env,
        st: &mut Vec<Value>,
    ) -> Result<(), Flow> {
        if let Some(value) = in_place {
            if appendable(env, var)
                && self.assign_in_place(var.s, var.slot, var.name, value, env)?
            {
                return Ok(());
            }
        }
        let (l, r) = self.operands(l, r, env, st)?;
        let v = binop_operands(op, l, r)?;
        store(env, var, v)
    }

    /// An `if`/`while` compare when the scalar fast path declined: the
    /// general operands and `binop_operands`, then `cond_bool` unless the
    /// result is a plain bool.
    #[inline(never)]
    fn cmp_slow(
        &self,
        op: &BinOp,
        l: &Opnd<'_>,
        r: &Opnd<'_>,
        cond: Cond,
        env: &Env,
        st: &mut Vec<Value>,
    ) -> Result<bool, Flow> {
        let (l, r) = self.operands(l, r, env, st)?;
        match binop_operands(op, l, r)? {
            Value::Bool(b) => Ok(b),
            v => cond_bool(v, cond.word()),
        }
    }

    /// Read the operands of a binary op, the left one first (a `Stack` right
    /// operand was pushed last, so it is popped first).
    #[inline(always)]
    fn operands(
        &self,
        l: &Opnd<'_>,
        r: &Opnd<'_>,
        env: &Env,
        st: &mut Vec<Value>,
    ) -> Result<(Operand, Operand), Flow> {
        let rv = match r {
            Opnd::Stack => Some(pop(st)),
            _ => None,
        };
        let lo = self.operand_of(l, env, st)?;
        let ro = match rv {
            Some(v) => Operand::of(v),
            None => self.operand_of(r, env, st)?,
        };
        Ok((lo, ro))
    }

    /// One operand, as `eval_binop`'s `operand` reads it.
    #[inline(always)]
    fn operand_of(&self, o: &Opnd<'_>, env: &Env, st: &mut Vec<Value>) -> Result<Operand, Flow> {
        Ok(match o {
            Opnd::Local(var) => match env.get_var(var.s, var.slot) {
                Some(v) => Operand::of_ref(v),
                None => Operand::of(self.ident_unbound(var.name, var.s)?),
            },
            Opnd::Int(n) => Operand::Int(*n),
            Opnd::Float(f) => Operand::Float(*f),
            Opnd::Const(v) => Operand::of_ref(v),
            Opnd::Stack => Operand::of(pop(st)),
        })
    }

    /// The `Index` arm's existence test: `name` is bound locally or in
    /// `globals`, so `name[i]` reads it in place.
    #[inline(always)]
    fn bound_in_place(&self, var: &Var<'_>, env: &Env) -> bool {
        env.get_var(var.s, var.slot).is_some() || self.globals.contains_key(&var.s)
    }

    /// The value an inline operand evaluates to, as `eval` evaluates its
    /// node (an identifier through the `Ident` arm's whole chain).
    fn opnd_value(&self, o: &Opnd<'_>, env: &Env) -> R {
        match o {
            Opnd::Local(var) => match env.get_var(var.s, var.slot) {
                Some(v) => Ok(v.clone()),
                None => self.ident_unbound(var.name, var.s),
            },
            Opnd::Int(n) => Ok(Value::Int(*n)),
            Opnd::Float(f) => Ok(Value::Float(*f)),
            Opnd::Const(v) => Ok(v.clone()),
            Opnd::Stack => malformed(),
        }
    }

    /// [`Op::BranchIndexLocal`] and [`Op::BranchIndexInt`] off their fast
    /// path: push the element as [`Op::IndexLocal`] reads it, then
    /// [`Op::BranchCmp`]'s path with a stack left operand.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn branch_index_slow(
        &self,
        op: &BinOp,
        arr: &Var<'_>,
        idx: &Opnd<'_>,
        r: &Opnd<'_>,
        cond: Cond,
        env: &Env,
        st: &mut Vec<Value>,
    ) -> Result<bool, Flow> {
        let v = match index_fast(env, arr, idx) {
            Some(v) => v,
            None => self.index_local_slow(arr, idx, env)?,
        };
        st.push(v);
        let l = Opnd::Stack;
        match scalar_fast(op, &l, r, env, st) {
            Some(Scalar::Bool(b)) => Ok(b),
            Some(s) => cond_bool(s.value(), cond.word()),
            None => self.cmp_slow(op, &l, r, cond, env, st),
        }
    }

    /// [`Op::IndexLocal`] when [`index_fast`] declined: the `Index` arm's
    /// two paths with the index node evaluated where the arm evaluates it.
    #[inline(never)]
    fn index_local_slow(&self, arr: &Var<'_>, idx: &Opnd<'_>, env: &Env) -> R {
        if self.bound_in_place(arr, env) {
            let i = strict_int(self.opnd_value(idx, env)?)?;
            self.index_in_place(arr.s, arr.slot, arr.name, i, env)
        } else {
            let recv = self.ident_unbound(arr.name, arr.s)?;
            let i = strict_int(self.opnd_value(idx, env)?)?;
            index_value(recv, i)
        }
    }

    /// An inline index of a place being written, through `place_index`.
    #[inline(always)]
    fn place_index_opnd(&self, o: &Opnd<'_>, env: &Env) -> Result<usize, Flow> {
        match o {
            Opnd::Int(n) if *n >= 0 => Ok(*n as usize),
            Opnd::Local(var) => match env.get_var(var.s, var.slot) {
                Some(v) => place_index(v),
                None => place_index(&self.ident_unbound(var.name, var.s)?),
            },
            o => place_index(&self.opnd_value(o, env)?),
        }
    }

    /// [`Op::WriteIndexLocal`] once its value `v` is read: the index, then
    /// the element set in place when `base` holds a uniquely owned array
    /// and the index is in its bounds, else `write_place` with that one
    /// step.
    #[inline(always)]
    fn write_index_local(
        &self,
        base: &Var<'_>,
        idx: &Opnd<'_>,
        v: Value,
        env: &mut Env,
    ) -> Result<(), Flow> {
        let i = self.place_index_opnd(idx, env)?;
        match write_index_unique(env, base, i, v) {
            None => Ok(()),
            Some(v) => write_place(base.s, base.slot, &[PlaceStep::Index(i)], v, env),
        }
    }

    /// S5: a fast call's body that is one `base[idx] = val` on locals and
    /// literals, its value `()` (an [`Op::WriteIndexLocal`] with inline
    /// operands, then `Const(Unit)`), runs without an activation of the op
    /// loop: the op's steps, panics included, then `()` (cost only). `None`
    /// for any other body, and for one that names `goal_met`, which a fast
    /// call binds only when the body runs on the op loop (as
    /// [`Body::leaf_value`]).
    #[inline(always)]
    fn leaf_store(&self, body: &Body<'_>, env: &mut Env) -> Option<R> {
        let [Op::WriteIndexLocal { base, idx, val }, Op::Const(Value::Unit)] = &*body.ops else {
            return None;
        };
        let declines = |o: &Opnd<'_>| match o {
            Opnd::Local(v) => v.s == SYM_GOAL_MET,
            Opnd::Stack => true,
            _ => false,
        };
        if base.s == SYM_GOAL_MET || declines(idx) || declines(val) {
            return None;
        }
        let v = match val {
            Opnd::Int(n) => Value::Int(*n),
            Opnd::Local(var) => match env.get_var(var.s, var.slot) {
                Some(Value::Int(n)) => Value::Int(*n),
                Some(v) => v.clone(),
                None => match self.ident_unbound(var.name, var.s) {
                    Ok(v) => v,
                    Err(flow) => return Some(Err(flow)),
                },
            },
            o => match self.opnd_value(o, env) {
                Ok(v) => v,
                Err(flow) => return Some(Err(flow)),
            },
        };
        Some(
            self.write_index_local(base, idx, v, env)
                .map(|()| Value::Unit),
        )
    }

    /// [`Op::Record`]: the literal's resolution (else resolved now, as the
    /// `StructLit` arm does for a node outside the table), then its `empty`
    /// value or `finish_record` over the field values on top of the stack.
    #[inline(never)]
    fn record_op(&self, e: &Expr, st: &mut Vec<Value>) -> R {
        let Expr::StructLit { name, fields } = e else {
            malformed()
        };
        let at = tail(st, fields.len() as u32);
        let unresolved;
        let lit = match self.res.record_lit(e) {
            Some(lit) => lit,
            None => {
                unresolved = RecordLit::of(name, fields, &self.structs, &self.enums);
                &unresolved
            }
        };
        let v = match &lit.empty {
            Some(v) => Ok(v.clone()),
            None => self.finish_record(lit, name, &mut st[at..]),
        };
        st.truncate(at);
        v
    }

    /// [`Op::CallFast`] before its first fast call: whether `call_builtin`
    /// is proven not to claim `callee` (`dispatch_named` cached it as
    /// `fn_table[entry]`, which only an inert non-builtin name gets). Once
    /// it is, `proven` keeps it: the cache entry never changes.
    #[inline(never)]
    fn fast_call_proven(&self, callee: &Var<'_>, entry: u32, proven: &Cell<bool>) -> bool {
        let known = self.callees.borrow().get(callee.s.index()).copied();
        let ok = known == Some(CALLEE_FN + entry);
        proven.set(ok);
        ok
    }

    /// [`Op::CallFast`] before `proven`: the arguments on the stack (inline
    /// ones pushed now, in order, as the `Ident`/`Literal` arms evaluate
    /// them), then `dispatch_named`, which caches the callee.
    #[inline(never)]
    fn call_unproven(
        &self,
        callee: &Var<'_>,
        argc: u32,
        args: &[Opnd<'_>],
        env: &mut Env,
        st: &mut Vec<Value>,
    ) -> R {
        let base = st.len();
        for a in args {
            match self.opnd_value(a, env) {
                Ok(v) => st.push(v),
                Err(flow) => {
                    st.truncate(base);
                    return Err(flow);
                }
            }
        }
        let Some(at) = st.len().checked_sub(argc as usize) else {
            malformed()
        };
        let argv = StackTail { st, at };
        self.dispatch_named(callee.name, callee.s, callee.slot, argv, None, env)
    }

    /// [`Op::CallFast`] once proven (spec §4 S5): `call_fn_entry` →
    /// `call_fn_in`'s common path for `entry` without the dispatch, its
    /// arguments the top of `st` from `at`. In the tree's order: the `tier:`
    /// clear `dispatch_call` makes before the call, a pooled frame, the depth
    /// check and the `current_fn` swap, the arity check, each parameter
    /// moved off the stack (soft unwrap, sized coercion), then
    /// [`Interp::fast_body`]. The arguments leave the stack on every path;
    /// `CallGuard` and `give_frame` restore the rest.
    #[inline(never)]
    fn call_fast(&self, entry: &FnEntry<'p>, st: &mut Vec<Value>, at: usize) -> R {
        debug_assert!(compile::fast_call_blocker(entry, self.refine_preds.is_empty()).is_none());
        self.set_call_tier(None);
        let mut frame = self.take_frame();
        let out = match self.enter_fn(entry.def) {
            Ok(_guard) => {
                let argc = st.len() - at;
                if entry.params.len() != argc {
                    st.truncate(at);
                    arity_mismatch(entry.def, argc)
                } else {
                    let env = &mut *frame;
                    debug_assert!(env.vars.is_empty() && env.marks.is_empty());
                    if argc == 1 {
                        let a = pop(st);
                        env.vars.push((entry.params[0], param_value(entry, 0, a)));
                    } else {
                        for (i, a) in st.drain(at..).enumerate() {
                            env.vars.push((entry.params[i], param_value(entry, i, a)));
                        }
                    }
                    self.fast_body(entry, env, st)
                }
            }
            Err(flow) => {
                st.truncate(at);
                Err(flow)
            }
        };
        self.give_frame(frame);
        out
    }

    /// [`Op::CallFast`] once proven, its arguments inline operands (as many
    /// as `entry` has parameters, so the arity check cannot fail). They are
    /// read first, as the tree evaluates them before the dispatch, and bound
    /// straight into the pooled frame (soft unwrap, sized coercion: pure, so
    /// doing it before the depth check is unobservable); then the `tier:`
    /// clear, the depth check and the `current_fn` swap, and
    /// [`Interp::fast_body`]. A leaf body ([`Interp::leaf_call`]) is
    /// computed without a frame.
    #[inline(never)]
    fn call_fast_inline(
        &self,
        entry: &FnEntry<'p>,
        args: &[Opnd<'_>],
        env: &Env,
        st: &mut Vec<Value>,
    ) -> R {
        debug_assert!(compile::fast_call_blocker(entry, self.refine_preds.is_empty()).is_none());
        debug_assert_eq!(args.len(), entry.params.len());
        if let Some(v) = self.leaf_call(entry, args, env) {
            return Ok(v);
        }
        let mut frame = self.take_frame();
        for (i, (a, &p)) in args.iter().zip(entry.params.iter()).enumerate() {
            let v = match a {
                Opnd::Local(var) => match env.get_var(var.s, var.slot) {
                    Some(Value::Int(n)) => Value::Int(*n),
                    Some(v) => v.clone(),
                    None => match self.ident_unbound(var.name, var.s) {
                        Ok(v) => v,
                        Err(flow) => {
                            self.give_frame(frame);
                            return Err(flow);
                        }
                    },
                },
                Opnd::Int(n) => Value::Int(*n),
                Opnd::Float(f) => Value::Float(*f),
                Opnd::Const(v) => v.clone(),
                Opnd::Stack => malformed(),
            };
            frame.vars.push((p, param_value(entry, i, v)));
        }
        self.set_call_tier(None);
        let out = match self.enter_fn(entry.def) {
            Ok(_guard) => self.fast_body(entry, &mut frame, st),
            Err(flow) => Err(flow),
        };
        self.give_frame(frame);
        out
    }

    /// A fast call (inline arguments) whose body is a leaf: one `Load` of a
    /// parameter, or one int/float `Bin` on parameters and literals. Its
    /// value is computed from the arguments as the parameters bind them
    /// (soft unwrap, sized coercion), without a frame, `goal_met` or
    /// `current_fn` swap: nothing runs in the callee that could observe
    /// them, and it cannot fail. The depth check and the `tier:` clear stay
    /// (a call one past the limit takes the full path and panics there).
    /// `None`, with nothing changed, wherever the full path could differ:
    /// a body not compiled yet or not a leaf, more than two parameters, an
    /// unbound argument (the full path panics on it, in order), a name that
    /// is not a parameter's slot or is `goal_met`, an operand that is not an
    /// int or float, overflow or a zero divisor (cost only, S5).
    #[inline(always)]
    fn leaf_call(&self, entry: &FnEntry<'p>, args: &[Opnd<'_>], env: &Env) -> Option<Value> {
        let body = entry.compiled.as_ref()?.get()?;
        if args.len() > 2 || self.call_depth.get() >= self.max_depth {
            return None;
        }
        // Parameter `k`'s value as `bind_params` binds it.
        let param = |var: &Var<'_>| -> Option<Value> {
            let k = var.slot as usize;
            if var.s == SYM_GOAL_MET || entry.params.get(k) != Some(&var.s) {
                return None;
            }
            let a = match &args[k] {
                Opnd::Local(a) => match env.get_var(a.s, a.slot)? {
                    Value::Int(n) => Value::Int(*n),
                    v => v.clone(),
                },
                Opnd::Int(n) => Value::Int(*n),
                Opnd::Float(f) => Value::Float(*f),
                Opnd::Const(v) => v.clone(),
                Opnd::Stack => malformed(),
            };
            Some(param_value(entry, k, a))
        };
        // Every argument must be bound, read or not.
        for a in args {
            if let Opnd::Local(a) = a {
                env.get_var(a.s, a.slot)?;
            }
        }
        let scalar = |o: &Opnd<'_>| -> Option<Scalar> {
            let v = match o {
                Opnd::Int(n) => return Some(Scalar::Int(*n)),
                Opnd::Float(f) => return Some(Scalar::Float(*f)),
                Opnd::Local(var) => param(var)?,
                _ => return None,
            };
            match v {
                Value::Int(n) => Some(Scalar::Int(n)),
                Value::Float(f) => Some(Scalar::Float(f)),
                v => {
                    drop(v);
                    None
                }
            }
        };
        let v = match &*body.ops {
            [Op::Load(var)] => param(var)?,
            [Op::Bin { op, l, r }] => match (scalar(l)?, scalar(r)?) {
                (Scalar::Int(a), Scalar::Int(b)) => int_fast(op, a, b)?.value(),
                (Scalar::Float(a), Scalar::Float(b)) => float_fast(op, a, b)?.value(),
                _ => return None,
            },
            _ => return None,
        };
        self.set_call_tier(None);
        Some(scalar_return(entry, v))
    }

    /// The rest of a fast call once the parameters are bound, as
    /// `call_fn_common` does it: `goal_met`, the body (a leaf body,
    /// [`Body::leaf_value`] or [`Interp::leaf_store`], without an activation
    /// of the op loop, any other on the caller's stack `st`,
    /// [`Interp::exec_on`]), the scalar return.
    #[inline(always)]
    fn fast_body(&self, entry: &FnEntry<'p>, env: &mut Env, st: &mut Vec<Value>) -> R {
        let out = match entry.compiled.as_ref().and_then(std::cell::OnceCell::get) {
            // A leaf body never sees `goal_met` (it declines one that names
            // it), so it is not bound for one (cost only).
            Some(body) => match body.leaf_value(env, true) {
                Some(v) => return Ok(scalar_return(entry, v)),
                None => match self.leaf_store(body, env) {
                    Some(out) => out,
                    None => {
                        // `goal_met` follows the parameters, as in `call_fn_common`.
                        env.vars.push((SYM_GOAL_MET, Value::Int(0)));
                        self.exec_on(body, env, st)
                    }
                },
            },
            None => {
                env.vars.push((SYM_GOAL_MET, Value::Int(0)));
                self.run_body_vm(entry, env)
            }
        };
        match out {
            Ok(v) | Err(Flow::Return(v)) => Ok(scalar_return(entry, v)),
            Err(other) => Err(other),
        }
    }

    /// [`Op::CallMut`]: `call_mut_with` → `call_fn_in`'s steps for
    /// `fn_table[entry]`, one operand per argument (`args`, the `stacked`
    /// stack ones the top of `st`) and the borrowed bindings `refs`. For a
    /// fn `call_fn_in` sends down its common path (`plain`, no refinements
    /// in the program, as many arguments as parameters) the arguments bind
    /// straight into the pooled frame: every plain one first, read in order
    /// (an unbound name panics as the `Ident` arm does, nothing borrowed
    /// yet), then each borrowed local moved out of its binding in argument
    /// order; a borrowed name that is not bound panics with
    /// `call_mut_with`'s text, the bindings moved before it left `Unit` as
    /// there. Then the `tier:` slot, the depth check and `current_fn` swap,
    /// `goal_met`, the body and the scalar return ([`Interp::fast_body`]),
    /// and each `&mut` param's final value moved back. A call that never
    /// binds its params (the depth check failed) moves nothing back: the
    /// borrowed bindings stay `Unit`, as `call_mut_with` leaves them. Any
    /// other fn takes [`Interp::call_mut_general`].
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn call_mut_op(
        &self,
        call: &Expr,
        entry: Option<u32>,
        args: &[Opnd<'_>],
        stacked: u32,
        refs: &[MutRef<'_>],
        env: &mut Env,
        st: &mut Vec<Value>,
    ) -> R {
        let Expr::Call {
            callee,
            args: arg_nodes,
            tier,
        } = call
        else {
            unreachable!("`CallMut` is compiled from a call")
        };
        let Some(entry) = entry else {
            return self.call_mut(callee, arg_nodes, tier.as_deref(), env);
        };
        let entry = &self.fn_table[entry as usize];
        let argc = args.len();
        let Some(at) = st.len().checked_sub(stacked as usize) else {
            malformed()
        };
        if !entry.plain || !self.refine_preds.is_empty() || entry.params.len() != argc {
            return self.call_mut_general(entry, arg_nodes, args, tier.as_deref(), env, st, at);
        }
        let mut frame = self.take_frame();
        frame.vars.reserve(argc + 1);
        let mut next_plain = at;
        for (i, o) in args.iter().enumerate() {
            let v = match o {
                Opnd::Stack => {
                    let v = std::mem::replace(&mut st[next_plain], Value::Unit);
                    next_plain += 1;
                    v
                }
                Opnd::Int(n) => Value::Int(*n),
                // A `&mut` position's placeholder.
                Opnd::Const(Value::Unit) => Value::Unit,
                Opnd::Local(var) => match env.get_var(var.s, var.slot) {
                    Some(Value::Int(n)) => Value::Int(*n),
                    Some(v) => v.clone(),
                    // Not a local: a global's value, or the unbound panic.
                    None => match self.ident_unbound(var.name, var.s) {
                        Ok(v) => v,
                        Err(flow) => {
                            self.give_frame(frame);
                            return Err(flow);
                        }
                    },
                },
                o => match self.opnd_value(o, env) {
                    Ok(v) => v,
                    Err(flow) => {
                        self.give_frame(frame);
                        return Err(flow);
                    }
                },
            };
            frame.vars.push((entry.params[i], param_value(entry, i, v)));
        }
        // Every stack argument moved out, leaving a `Unit`: nothing to drop.
        while st.len() > at {
            forget_scalar(pop(st));
        }
        // Each `&mut` position holds a `Unit` placeholder until its
        // borrowed binding moves in.
        for r in refs {
            let Some(b) = env.get_var_mut(r.var.s, r.var.slot) else {
                self.give_frame(frame);
                let name = r.var.name;
                return panic(format!("`&mut {name}`: `{name}` is not a local variable"));
            };
            let v = param_value(entry, r.arg as usize, std::mem::replace(b, Value::Unit));
            forget_scalar(std::mem::replace(&mut frame.vars[r.arg as usize].1, v));
        }
        self.set_call_tier(tier.as_deref());
        let result = match self.enter_fn(entry.def) {
            Ok(_guard) => self.fast_body(entry, &mut frame, st),
            Err(flow) => {
                self.give_frame(frame);
                return Err(flow);
            }
        };
        // The body's scopes are popped, so the frame holds the params and
        // `goal_met` alone (param `i` at slot `i`; a leaf body binds no
        // `goal_met`) unless a `define` at the base scope added a binding;
        // then the name lookup finds it as `call_mut_with`'s does.
        let exact = frame.vars.len() <= argc + 1;
        for r in refs {
            let out = match r.back {
                true if exact => std::mem::replace(&mut frame.vars[r.arg as usize].1, Value::Unit),
                true => match frame.get_mut(entry.params[r.arg as usize]) {
                    Some(v) => std::mem::replace(v, Value::Unit),
                    None => Value::Unit,
                },
                false => Value::Unit,
            };
            if let Some(b) = env.get_var_mut(r.var.s, r.var.slot) {
                forget_scalar(std::mem::replace(b, out));
            }
        }
        self.give_frame(frame);
        result
    }

    /// [`Op::CallMut`] for a fn off `call_fn_in`'s common path: the plain
    /// arguments (the stack ones from `at`, the inline ones read in order)
    /// and a `Unit` in each `&mut` position, in a pooled buffer, then
    /// `call_mut_with` as the tree-walker runs it.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)]
    fn call_mut_general(
        &self,
        entry: &FnEntry<'p>,
        arg_nodes: &[Expr],
        args: &[Opnd<'_>],
        tier: Option<&str>,
        env: &mut Env,
        st: &mut Vec<Value>,
        at: usize,
    ) -> R {
        let mut argv = self.take_args(args.len());
        let mut plain = st.drain(at..);
        for o in args {
            argv.push(match o {
                Opnd::Stack => match plain.next() {
                    Some(v) => v,
                    None => malformed(),
                },
                o => self.opnd_value(o, env)?,
            });
        }
        drop(plain);
        self.call_mut_with(entry, arg_nodes, argv, tier, env)
    }

    /// `vm: tree <name>: <reason>` under `AXON_ENGINE=vm AXON_VM_TRACE=1`: a
    /// whole fn body runs on the tree-walker.
    pub(super) fn vm_trace_tree(&self, entry: &FnEntry<'_>, reason: &str) {
        if self.engine == Engine::Vm && self.vm_trace {
            eprintln!("vm: tree {}: {reason}", self.vm_body_name(entry));
        }
    }

    /// The per-body trace line, one `tree-op` line per `Tree` op and one
    /// `slow` line per call a flag keeps off the fast path (S5), printed
    /// when a body is compiled (its first run).
    fn vm_trace_compiled(&self, entry: &FnEntry<'_>, body: &Body<'_>) {
        self.vm_trace_named(&self.vm_body_name(entry), body);
    }

    /// [`Interp::vm_trace_compiled`] for the body named `name` (a fn's or a
    /// lambda's, spec §3).
    fn vm_trace_named(&self, name: &str, body: &Body<'_>) {
        eprintln!(
            "vm: {name} {} ops, {} tree nodes",
            body.ops.len(),
            body.tree_nodes()
        );
        let (p, q) = body.pure_ops();
        if p + q > 0 {
            eprintln!("vm: pure {name} {p} exprs, {q} loops");
        }
        for op in body.ops.iter() {
            match op {
                Op::Tree(e) => {
                    let variant = compile::variant_name(e);
                    match compile::tree_shape(e) {
                        Some(shape) => eprintln!("vm: tree-op {name} {variant}({shape})"),
                        None => eprintln!("vm: tree-op {name} {variant}"),
                    }
                }
                Op::Call {
                    callee,
                    slow: Some(flag),
                    ..
                } => eprintln!("vm: slow {}: {flag}", callee.name),
                _ => {}
            }
        }
    }

    /// A body's trace name: the fn's name, `Type::method` for an impl method.
    fn vm_body_name(&self, entry: &FnEntry<'_>) -> String {
        self.methods
            .iter()
            .find(|(_, d)| std::ptr::eq(**d, entry.def))
            .map(|((ty, m), _)| format!("{ty}::{m}"))
            .unwrap_or_else(|| entry.def.name.clone())
    }
}
