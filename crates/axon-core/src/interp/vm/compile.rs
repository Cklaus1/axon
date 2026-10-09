//! R50: the compiler from a fn body to a [`Body`]. Through S0 no construct is
//! lowered: every variant takes its explicit `Tree` arm, so a body compiles to
//! one `Tree` op over its root. The `match` names every `Expr` variant (no
//! wildcard): a new variant does not compile until someone decides how the
//! engine runs it (spec §4 "Lowered set per slice").

use super::{Body, Op};
use crate::ast::{Expr, UnaryOp};

/// Compile the fn body `body`.
pub(in crate::interp) fn compile(body: &Expr) -> Body<'_> {
    let mut c = Compiler { ops: Vec::new() };
    c.expr(body);
    Body {
        ops: c.ops.into_boxed_slice(),
    }
}

struct Compiler<'p> {
    ops: Vec<Op<'p>>,
}

impl<'p> Compiler<'p> {
    /// Emit the ops that evaluate `e` and push its value.
    fn expr(&mut self, e: &'p Expr) {
        match e {
            Expr::Block(_) => self.tree(e),
            Expr::Let { .. } => self.tree(e),
            Expr::Own { .. } => self.tree(e),
            Expr::RefBind { .. } => self.tree(e),
            Expr::Call { .. } => self.tree(e),
            Expr::MethodCall { .. } => self.tree(e),
            Expr::BinOp { .. } => self.tree(e),
            Expr::UnaryOp { .. } => self.tree(e),
            Expr::Question(_) => self.tree(e),
            Expr::Match { .. } => self.tree(e),
            Expr::If { .. } => self.tree(e),
            Expr::Spawn(_) => self.tree(e),
            Expr::Select(_) => self.tree(e),
            Expr::Comptime(_) => self.tree(e),
            Expr::InlineAsm { .. } => self.tree(e),
            Expr::Lambda { .. } => self.tree(e),
            Expr::Return(_) => self.tree(e),
            Expr::FieldAccess { .. } => self.tree(e),
            Expr::Index { .. } => self.tree(e),
            Expr::Tuple(_) => self.tree(e),
            Expr::Ident(_) => self.tree(e),
            Expr::Literal(_) => self.tree(e),
            Expr::FmtStr { .. } => self.tree(e),
            Expr::Ok(_) => self.tree(e),
            Expr::Err(_) => self.tree(e),
            Expr::Some(_) => self.tree(e),
            Expr::None => self.tree(e),
            Expr::Array(_) => self.tree(e),
            Expr::StructLit { .. } => self.tree(e),
            Expr::While { .. } => self.tree(e),
            Expr::WhileLet { .. } => self.tree(e),
            Expr::Assign { .. } => self.tree(e),
            Expr::WithHandler { .. } => self.tree(e),
            Expr::AssignTo { .. } => self.tree(e),
            Expr::Break => self.tree(e),
            Expr::Continue => self.tree(e),
            Expr::For { .. } => self.tree(e),
        }
    }

    /// `e` runs on the tree-walker as one op.
    fn tree(&mut self, e: &'p Expr) {
        self.ops.push(Op::Tree(e));
    }
}

/// The `Expr` variant's name, as the `vm: tree-op` trace line prints it.
pub(super) fn variant_name(e: &Expr) -> &'static str {
    match e {
        Expr::Block(_) => "Block",
        Expr::Let { .. } => "Let",
        Expr::Own { .. } => "Own",
        Expr::RefBind { .. } => "RefBind",
        Expr::Call { .. } => "Call",
        Expr::MethodCall { .. } => "MethodCall",
        Expr::BinOp { .. } => "BinOp",
        Expr::UnaryOp { .. } => "UnaryOp",
        Expr::Question(_) => "Question",
        Expr::Match { .. } => "Match",
        Expr::If { .. } => "If",
        Expr::Spawn(_) => "Spawn",
        Expr::Select(_) => "Select",
        Expr::Comptime(_) => "Comptime",
        Expr::InlineAsm { .. } => "InlineAsm",
        Expr::Lambda { .. } => "Lambda",
        Expr::Return(_) => "Return",
        Expr::FieldAccess { .. } => "FieldAccess",
        Expr::Index { .. } => "Index",
        Expr::Tuple(_) => "Tuple",
        Expr::Ident(_) => "Ident",
        Expr::Literal(_) => "Literal",
        Expr::FmtStr { .. } => "FmtStr",
        Expr::Ok(_) => "Ok",
        Expr::Err(_) => "Err",
        Expr::Some(_) => "Some",
        Expr::None => "None",
        Expr::Array(_) => "Array",
        Expr::StructLit { .. } => "StructLit",
        Expr::While { .. } => "While",
        Expr::WhileLet { .. } => "WhileLet",
        Expr::Assign { .. } => "Assign",
        Expr::WithHandler { .. } => "WithHandler",
        Expr::AssignTo { .. } => "AssignTo",
        Expr::Break => "Break",
        Expr::Continue => "Continue",
        Expr::For { .. } => "For",
    }
}

/// The `<shape>` of a `vm: tree-op` line (spec §3, §8): the exceptions inside
/// a variant that is (or will be) lowered. `None` for every other node.
pub(super) fn tree_shape(e: &Expr) -> Option<&'static str> {
    match e {
        // In `eval`'s order: the `StructLit` callee forms, then `P(..)`, then
        // the `&mut` protocol, then a callee that is not an identifier.
        Expr::Call { callee, args, .. } => match callee.as_ref() {
            Expr::StructLit { .. } => Some("struct-lit"),
            Expr::Ident(n) if n == "P" && args.len() == 1 => Some("P"),
            _ if args.iter().any(|a| {
                matches!(
                    a,
                    Expr::UnaryOp {
                        op: UnaryOp::RefMut,
                        ..
                    }
                )
            }) =>
            {
                Some("&mut")
            }
            Expr::Ident(_) => None,
            _ => Some("computed"),
        },
        Expr::Index { receiver, .. } if matches!(receiver.as_ref(), Expr::Ident(n) if n == "E" || n == "Var") => {
            Some("E|Var")
        }
        _ => None,
    }
}
