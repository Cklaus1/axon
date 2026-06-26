//! AST description-length metric (`axon complexity`).
//!
//! A minimum-description-length (MDL) approximation over the typed AST: the
//! number of *bits* it takes to describe the program, summed structurally. This
//! is the "measure of simplest program" a compression loop needs — the fitness
//! function for a future `goal { minimize complexity, subject_to: fits_obs }`
//! (the world-model / skill-acquisition pattern). Because it is a pure fold over
//! the AST (not the source text) it is:
//!   * **deterministic** — same AST → same bits (gate-safe, no Date/random);
//!   * **format-invariant** — whitespace/comments don't change the score;
//!   * **monotone** — adding a node strictly increases the bit count.
//!
//! v1 is an explicit, principled APPROXIMATION, not an information-theoretic
//! optimum: each node pays a fixed "which kind" cost plus content costs
//! (literal magnitude, name length, type annotations). The point is a sound,
//! minimizable scalar — a smaller score is a genuinely simpler hypothesis, not
//! just terser text — rather than an exact Kolmogorov complexity (uncomputable).

use crate::ast::*;

/// The "which kind of node is this" cost in bits. The `Expr` enum has ~40
/// variants, so naming one costs `ceil(log2(40)) = 6` bits. A fixed constant
/// keeps the metric stable as variants are added (it only shifts the constant,
/// never the relative ordering of programs).
const KIND_BITS: u64 = 6;

/// Bits to describe one character of an identifier/name. A name is description
/// the reader must store, so longer names cost more — the MDL-honest version of
/// "least code that still verifies": a terse name is cheaper only until it stops
/// being load-bearing. 5 bits ≈ one lowercase-alpha+underscore symbol.
const NAME_BITS_PER_CHAR: u64 = 5;

/// A measured complexity: the headline `bits`, plus plain structural counts that
/// are cheap to compute and useful as context.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Complexity {
    /// Description length in bits (the minimizable scalar).
    pub bits: u64,
    /// Raw AST node count (exprs + stmts + items contributing a KIND cost).
    pub nodes: u64,
    /// Maximum expression-nesting depth.
    pub depth: u64,
}

/// Per-function complexity plus the program total.
#[derive(Debug, Clone, Default)]
pub struct ProgramComplexity {
    /// `(function_name, complexity)` in source order. Impl methods are named
    /// `Type::method`.
    pub functions: Vec<(String, Complexity)>,
    /// Whole-program total (all items, including type/refinement definitions).
    pub total: Complexity,
    /// Cost in bits attributed to each node kind, descending — shows WHERE the
    /// description length is (e.g. `Call`, `Ident`, `Literal`).
    pub by_kind: Vec<(&'static str, u64)>,
}

/// Number of bits to encode the magnitude of an integer literal.
fn int_bits(v: i64) -> u64 {
    let mag = (v as i128).unsigned_abs();
    // `n` distinct magnitudes need ceil(log2(n+1)) bits; +1 for the sign.
    (128 - mag.leading_zeros() as u64).max(1) + 1
}

fn name_bits(name: &str) -> u64 {
    NAME_BITS_PER_CHAR * name.chars().count() as u64
}

/// Accumulator that also tallies cost per node-kind.
#[derive(Default)]
struct Acc {
    bits: u64,
    nodes: u64,
    by_kind: std::collections::BTreeMap<&'static str, u64>,
}

impl Acc {
    fn add(&mut self, kind: &'static str, bits: u64) {
        self.bits += bits;
        self.nodes += 1;
        *self.by_kind.entry(kind).or_insert(0) += bits;
    }
}

/// Compute the complexity of a whole program.
pub fn program_complexity(program: &Program) -> ProgramComplexity {
    let mut out = ProgramComplexity::default();
    let mut acc = Acc::default();

    for item in &program.items {
        match item {
            Item::FnDef(f) => {
                let c = fn_complexity(f, &mut acc);
                out.functions.push((f.name.clone(), c));
            }
            Item::ImplBlock(blk) => {
                let tn = type_name_of(&blk.for_type);
                for m in &blk.methods {
                    let c = fn_complexity(m, &mut acc);
                    out.functions.push((format!("{tn}::{}", m.name), c));
                }
            }
            Item::LetDef { name, value, .. } => {
                acc.add("LetDef", name_bits(name));
                expr_cost(value, &mut acc, 1);
            }
            Item::RefineDef(r) => {
                acc.add("RefineDef", name_bits(&r.name));
                ty_cost(&r.base, &mut acc);
                expr_cost(&r.predicate, &mut acc, 1);
            }
            Item::TypeDef(t) => {
                acc.add("TypeDef", name_bits(&t.name));
                for field in &t.fields {
                    acc.add("Field", name_bits(&field.name));
                    ty_cost(&field.ty, &mut acc);
                }
                if let Some(pred) = &t.refinement {
                    expr_cost(pred, &mut acc, 1);
                }
            }
            Item::EnumDef(e) => {
                acc.add("EnumDef", name_bits(&e.name));
                for v in &e.variants {
                    acc.add("Variant", name_bits(&v.name));
                    for field in &v.fields {
                        ty_cost(&field.ty, &mut acc);
                    }
                }
            }
            Item::TraitDef(t) => {
                acc.add("TraitDef", name_bits(&t.name));
                for m in &t.methods {
                    acc.add("TraitMethod", name_bits(&m.name));
                }
            }
            // Module/use declarations are interface, not computation — count them
            // as a flat kind cost so they're not free, but they carry no body.
            Item::ModDecl(_) => acc.add("ModDecl", 0),
            Item::UseDecl(_) => acc.add("UseDecl", 0),
        }
    }

    out.total = Complexity {
        bits: acc.bits,
        nodes: acc.nodes,
        depth: 0,
    };
    // Depth is the max over functions (a program-level number isn't meaningful).
    out.total.depth = out
        .functions
        .iter()
        .map(|(_, c)| c.depth)
        .max()
        .unwrap_or(0);

    let mut by_kind: Vec<(&'static str, u64)> = acc.by_kind.into_iter().collect();
    by_kind.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    out.by_kind = by_kind;
    out
}

/// Complexity of a single function (signature + body). Adds into the shared
/// `acc` (for the program total + by-kind) AND returns the function's own
/// subtotal for the per-function table.
fn fn_complexity(f: &FnDef, acc: &mut Acc) -> Complexity {
    let before_bits = acc.bits;
    let before_nodes = acc.nodes;

    acc.add("FnDef", name_bits(&f.name));
    for p in &f.params {
        acc.add("Param", name_bits(&p.name));
        ty_cost(&p.ty, acc);
    }
    if let Some(rt) = &f.return_type {
        ty_cost(rt, acc);
    }
    let depth = expr_cost(&f.body, acc, 1);

    Complexity {
        bits: acc.bits - before_bits,
        nodes: acc.nodes - before_nodes,
        depth,
    }
}

/// Cost of a type annotation (description the reader must store). Returns nothing
/// useful; folds into `acc`.
fn ty_cost(t: &AxonType, acc: &mut Acc) {
    match t {
        AxonType::Named(n) | AxonType::TypeParam(n) | AxonType::DynTrait(n) => {
            acc.add("Type", name_bits(n))
        }
        AxonType::Option(i)
        | AxonType::Chan(i)
        | AxonType::Slice(i)
        | AxonType::Ref(i)
        | AxonType::RawPtr(i) => {
            acc.add("Type", KIND_BITS);
            ty_cost(i, acc);
        }
        AxonType::Result { ok, err } => {
            acc.add("Type", KIND_BITS);
            ty_cost(ok, acc);
            ty_cost(err, acc);
        }
        AxonType::Generic { base, args } => {
            acc.add("Type", name_bits(base));
            for a in args {
                ty_cost(a, acc);
            }
        }
        AxonType::Fn { params, ret } => {
            acc.add("Type", KIND_BITS);
            for p in params {
                ty_cost(p, acc);
            }
            ty_cost(ret, acc);
        }
        AxonType::Tuple(es) | AxonType::Union(es) => {
            acc.add("Type", KIND_BITS);
            for e in es {
                ty_cost(e, acc);
            }
        }
    }
}

/// Cost of a pattern (in match arms / handler bindings / while-let).
fn pat_cost(p: &Pattern, acc: &mut Acc) {
    match p {
        Pattern::Wildcard | Pattern::None => acc.add("Pattern", KIND_BITS),
        Pattern::Ident(n) => acc.add("Pattern", name_bits(n)),
        Pattern::Literal(l) => acc.add("Pattern", lit_bits(l)),
        Pattern::Some(i) | Pattern::Ok(i) | Pattern::Err(i) => {
            acc.add("Pattern", KIND_BITS);
            pat_cost(i, acc);
        }
        Pattern::Struct { name, fields } => {
            acc.add("Pattern", name_bits(name));
            for (fname, fp) in fields {
                acc.add("PatField", name_bits(fname));
                pat_cost(fp, acc);
            }
        }
        Pattern::Tuple(ps) => {
            acc.add("Pattern", KIND_BITS);
            for p in ps {
                pat_cost(p, acc);
            }
        }
    }
}

fn lit_bits(l: &Literal) -> u64 {
    match l {
        Literal::Int(v) => KIND_BITS + int_bits(*v),
        Literal::Float(_) => KIND_BITS + 64,
        Literal::Decimal(_) => KIND_BITS + 128, // i128 mantissa
        Literal::Bool(_) => KIND_BITS + 1,
        Literal::Str(s) => KIND_BITS + 8 * s.len() as u64,
    }
}

/// Fold one expression into `acc`; returns the max nesting depth reached.
fn expr_cost(e: &Expr, acc: &mut Acc, depth: u64) -> u64 {
    let mut max_depth = depth;
    // Helper to recurse a child and track depth.
    macro_rules! child {
        ($c:expr) => {{
            let d = expr_cost($c, acc, depth + 1);
            if d > max_depth {
                max_depth = d;
            }
        }};
    }

    match e {
        Expr::Block(stmts) => {
            acc.add("Block", KIND_BITS);
            for s in stmts {
                child!(&s.expr);
            }
        }
        Expr::Let { name, ty, value }
        | Expr::Own { name, ty, value }
        | Expr::RefBind { name, ty, value } => {
            acc.add("Let", name_bits(name));
            if let Some(t) = ty {
                ty_cost(t, acc);
            }
            child!(value);
        }
        Expr::Call { callee, args, .. } => {
            acc.add("Call", KIND_BITS);
            child!(callee);
            for a in args {
                child!(a);
            }
        }
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            acc.add("MethodCall", name_bits(method));
            child!(receiver);
            for a in args {
                child!(a);
            }
        }
        Expr::BinOp { left, right, .. } => {
            acc.add("BinOp", KIND_BITS);
            child!(left);
            child!(right);
        }
        Expr::UnaryOp { operand, .. } => {
            acc.add("UnaryOp", KIND_BITS);
            child!(operand);
        }
        Expr::Question(i) => {
            acc.add("Question", KIND_BITS);
            child!(i);
        }
        Expr::Match { subject, arms } => {
            acc.add("Match", KIND_BITS);
            child!(subject);
            for arm in arms {
                pat_cost(&arm.pattern, acc);
                if let Some(g) = &arm.guard {
                    child!(g);
                }
                child!(&arm.body);
            }
        }
        Expr::If { cond, then, else_ } => {
            acc.add("If", KIND_BITS);
            child!(cond);
            child!(then);
            if let Some(e) = else_ {
                child!(e);
            }
        }
        Expr::Spawn(i) => {
            acc.add("Spawn", KIND_BITS);
            child!(i);
        }
        Expr::Select(arms) => {
            acc.add("Select", KIND_BITS);
            for arm in arms {
                child!(&arm.recv);
                child!(&arm.body);
            }
        }
        Expr::Comptime(i) => {
            acc.add("Comptime", KIND_BITS);
            child!(i);
        }
        Expr::Lambda { params, body, .. } => {
            acc.add("Lambda", KIND_BITS);
            for p in params {
                acc.add("LambdaParam", name_bits(&p.name));
                if let Some(t) = &p.ty {
                    ty_cost(t, acc);
                }
            }
            child!(body);
        }
        Expr::Return(inner) => {
            acc.add("Return", KIND_BITS);
            if let Some(i) = inner {
                child!(i);
            }
        }
        Expr::FieldAccess { receiver, field } => {
            acc.add("FieldAccess", name_bits(field));
            child!(receiver);
        }
        Expr::Index { receiver, index } => {
            acc.add("Index", KIND_BITS);
            child!(receiver);
            child!(index);
        }
        Expr::Tuple(items) => {
            acc.add("Tuple", KIND_BITS);
            for it in items {
                child!(it);
            }
        }
        Expr::Ident(n) => acc.add("Ident", name_bits(n)),
        Expr::Literal(l) => acc.add("Literal", lit_bits(l)),
        Expr::FmtStr { parts } => {
            acc.add("FmtStr", KIND_BITS);
            for p in parts {
                match p {
                    FmtPart::Lit(s) => acc.add("FmtLit", 8 * s.len() as u64),
                    FmtPart::Expr(e) => child!(e),
                }
            }
        }
        Expr::Ok(i) | Expr::Err(i) | Expr::Some(i) => {
            acc.add("Ctor", KIND_BITS);
            child!(i);
        }
        Expr::None => acc.add("None", KIND_BITS),
        Expr::Array(items) => {
            acc.add("Array", KIND_BITS);
            for it in items {
                child!(it);
            }
        }
        Expr::StructLit { name, fields } => {
            acc.add("StructLit", name_bits(name));
            for (fname, v) in fields {
                acc.add("StructField", name_bits(fname));
                child!(v);
            }
        }
        Expr::While { cond, body } => {
            acc.add("While", KIND_BITS);
            child!(cond);
            for s in body {
                child!(&s.expr);
            }
        }
        Expr::WhileLet {
            pattern,
            expr,
            body,
        } => {
            acc.add("WhileLet", KIND_BITS);
            pat_cost(pattern, acc);
            child!(expr);
            for s in body {
                child!(&s.expr);
            }
        }
        Expr::Assign { name, value } => {
            acc.add("Assign", name_bits(name));
            child!(value);
        }
        Expr::WithHandler { handler, body } => {
            acc.add("WithHandler", KIND_BITS);
            match handler.as_ref() {
                HandlerExpr::Named(n) => acc.add("Handler", name_bits(n)),
                HandlerExpr::Inline { arms, return_arm } => {
                    for arm in arms {
                        acc.add("HandlerArm", name_bits(&arm.effect));
                        pat_cost(&arm.binding, acc);
                        child!(&arm.body);
                    }
                    if let Some(ra) = return_arm {
                        pat_cost(&ra.binding, acc);
                        child!(&ra.body);
                    }
                }
            }
            child!(body);
        }
        Expr::AssignTo { place, value } => {
            acc.add("AssignTo", KIND_BITS);
            child!(place);
            child!(value);
        }
        Expr::Break => acc.add("Break", KIND_BITS),
        Expr::Continue => acc.add("Continue", KIND_BITS),
        Expr::For {
            var,
            start,
            end,
            body,
            ..
        } => {
            acc.add("For", name_bits(var));
            child!(start);
            child!(end);
            for s in body {
                child!(&s.expr);
            }
        }
        Expr::InlineAsm { .. } => acc.add("InlineAsm", KIND_BITS),
    }

    max_depth
}

/// The simple name of a type (for `Type::method` labels), mirroring how impl
/// blocks are keyed elsewhere.
fn type_name_of(t: &AxonType) -> String {
    match t {
        AxonType::Named(n) | AxonType::TypeParam(n) | AxonType::DynTrait(n) => n.clone(),
        AxonType::Generic { base, .. } => base.clone(),
        _ => "_".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_source;

    fn bits(src: &str) -> u64 {
        program_complexity(&parse_source(src).expect("parse"))
            .total
            .bits
    }

    #[test]
    fn empty_program_is_zero() {
        assert_eq!(bits(""), 0);
    }

    #[test]
    fn adding_a_node_strictly_increases_bits() {
        let small = bits("fn f() -> i64 { 0 }");
        let bigger = bits("fn f() -> i64 { 0 + 1 }");
        assert!(bigger > small, "small={small} bigger={bigger}");
    }

    #[test]
    fn longer_names_cost_more() {
        let short = bits("fn f() -> i64 { let x = 1\n x }");
        let long = bits("fn f() -> i64 { let xxxxxxxxxx = 1\n xxxxxxxxxx }");
        assert!(long > short, "short={short} long={long}");
    }

    #[test]
    fn bigger_literals_cost_more() {
        let small = bits("fn f() -> i64 { 1 }");
        let big = bits("fn f() -> i64 { 1000000 }");
        assert!(big > small, "small={small} big={big}");
    }

    #[test]
    fn format_invariant() {
        // Same AST, different whitespace/comments → identical bits.
        let a = bits("fn f() -> i64 { 1 + 2 }");
        let b = bits("fn f() -> i64 {\n    // a comment\n    1 + 2\n}");
        assert_eq!(a, b, "formatting must not change the score: a={a} b={b}");
    }

    #[test]
    fn deterministic() {
        let src = "fn f(n: i64) -> i64 { if n > 0 { n } else { 0 } }";
        assert_eq!(bits(src), bits(src));
    }

    #[test]
    fn per_function_breakdown() {
        let src = "fn a() -> i64 { 0 }\nfn b() -> i64 { 1 + 2 + 3 }";
        let pc = program_complexity(&parse_source(src).expect("parse"));
        assert_eq!(pc.functions.len(), 2);
        let a = pc.functions.iter().find(|(n, _)| n == "a").unwrap().1.bits;
        let b = pc.functions.iter().find(|(n, _)| n == "b").unwrap().1.bits;
        assert!(b > a, "b should be more complex: a={a} b={b}");
        // The total is at least the sum of the functions (plus nothing else here).
        assert!(pc.total.bits >= a + b);
    }

    #[test]
    fn nesting_increases_depth() {
        let flat = program_complexity(&parse_source("fn f() -> i64 { 1 }").unwrap())
            .total
            .depth;
        let nested = program_complexity(
            &parse_source("fn f() -> i64 { if true { if true { 1 } else { 2 } } else { 3 } }")
                .unwrap(),
        )
        .total
        .depth;
        assert!(nested > flat, "flat={flat} nested={nested}");
    }
}
