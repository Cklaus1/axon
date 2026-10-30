//! Constraint-based type inference for Axon — Phase 1.
//!
//! Phase 1 uses concrete types only (no rank-N polymorphism, no generics).
//! The algorithm:
//! 1. `collect_sigs` — first pass to register all top-level fn/type/enum names.
//! 2. `infer_expr` — walk the AST, emit `Constraint`s, return inferred `Type`.
//! 3. `solve` — unify constraints via union-find; populate `Substitution`.
//!
//! Any remaining `Type::Var` after solving triggers an E0101 "cannot infer
//! type" error. Type mismatches during unification produce E0102.

use std::collections::{HashMap, HashSet};

use crate::ast::{
    AxonType, BinOp, Expr, FmtPart, FnDef, Item, Literal as AstLiteral, MatchArm, Pattern, Program,
    UnaryOp,
};
use crate::builtins::builtin_sigs;
use crate::types::{Constraint, Substitution, Type};

// ── Error codes ───────────────────────────────────────────────────────────────

const E0101: &str = "E0101";
const E0102: &str = "E0102";
const E0301: &str = "E0301";
const E0306: &str = "E0306";

/// AX-63: how many levels deep a type may nest on wasm32. wasm32 runs the front
/// end on wasmtime's default 512 KiB native stack, and the checker recurses over
/// types (clone, drop, `==`, unification, substitution). The source-nesting
/// limit (`parser::MAX_EXPR_DEPTH`, 224) does not bound that, because a type can
/// be deep while every source line is shallow: `let a1 = [a0]`, `let a2 = [a1]`,
/// … gives `a_n` the type `[[…[i64]…]]`, `n` levels deep, and 3,000 such lines
/// trapped the debug build. The same 224 budget, applied to types. Native keeps
/// its 1 GiB front-end stack and no limit, as for source nesting.
#[cfg(target_arch = "wasm32")]
pub(crate) const MAX_TYPE_DEPTH: Option<usize> = Some(224);
#[cfg(not(target_arch = "wasm32"))]
pub(crate) const MAX_TYPE_DEPTH: Option<usize> = None;
const E1900: &str = "E1900";
const E0404: &str = "E0404"; // enum-variant literal names a nonexistent variant/field

// ── Integer widening ──────────────────────────────────────────────────────────

/// True when `from` can be implicitly widened to `to` (i8 → i16 → i32 → i64).
fn is_int_widening(from: &Type, to: &Type) -> bool {
    let rank = |t: &Type| match t {
        Type::I8 => Some(0u8),
        Type::I16 => Some(1),
        Type::I32 => Some(2),
        Type::I64 => Some(3),
        _ => None,
    };
    matches!((rank(from), rank(to)), (Some(f), Some(t)) if f < t)
}

/// True when `t` is any fixed-width integer type (signed or unsigned). (R19)
fn is_int_type(t: &Type) -> bool {
    matches!(
        t,
        Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::U8 | Type::U16 | Type::U32 | Type::U64
    )
}

/// True when integer literal `n` is representable in integer type `ty`. (R19 §4
/// range-check; literal is `i64`, so `u64`'s only constraint is non-negativity.)
fn int_lit_in_range(n: i64, ty: &Type) -> bool {
    match ty {
        Type::I8 => (i8::MIN as i64..=i8::MAX as i64).contains(&n),
        Type::I16 => (i16::MIN as i64..=i16::MAX as i64).contains(&n),
        Type::I32 => (i32::MIN as i64..=i32::MAX as i64).contains(&n),
        Type::I64 => true,
        Type::U8 => (0..=u8::MAX as i64).contains(&n),
        Type::U16 => (0..=u16::MAX as i64).contains(&n),
        Type::U32 => (0..=u32::MAX as i64).contains(&n),
        Type::U64 => n >= 0,
        _ => false,
    }
}

/// Has this type not been pinned down yet?
///
/// Inference is constraint-based, so an argument's type at the CALL SITE is
/// frequently still a variable that only resolves when the constraints solve.
/// Anything that treats "not resolved yet" as "resolved to something wrong"
/// reports a mismatch against a type the value never had.
fn type_is_unresolved(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Var(_) | Type::Unknown | Type::TypeParam(_) | Type::Deferred(_)
    )
}

/// CS2-5: `ty` (a generic struct's field type, its params stored as
/// `Deferred("T")`) with each of the struct's type params `params` replaced by
/// the matching type argument in `args`, at any depth.
fn subst_struct_params(ty: Type, params: &[String], args: &[Type]) -> Type {
    ty.map_leaves(&mut |t| match &t {
        Type::Deferred(n) | Type::TypeParam(n) => params
            .iter()
            .position(|p| p == n)
            .and_then(|i| args.get(i).cloned())
            .unwrap_or(t),
        _ => t,
    })
}

/// CS2-3b: the bare `Dict`s (no `<str, V>`) written in a type, the ones
/// `resolve_ast_type` gives an unwritten value (`Dict(Unknown)`).
fn count_bare_dicts(t: &AxonType) -> usize {
    match t {
        AxonType::Named(n) => usize::from(n == "Dict"),
        AxonType::Result { ok, err } => count_bare_dicts(ok) + count_bare_dicts(err),
        AxonType::Option(i)
        | AxonType::Chan(i)
        | AxonType::Slice(i)
        | AxonType::Ref(i)
        | AxonType::RefMut(i)
        | AxonType::RawPtr(i) => count_bare_dicts(i),
        AxonType::Generic { args, .. } | AxonType::Tuple(args) => {
            args.iter().map(count_bare_dicts).sum()
        }
        AxonType::Fn { params, ret } => {
            params.iter().map(count_bare_dicts).sum::<usize>() + count_bare_dicts(ret)
        }
        AxonType::DynTrait(_) | AxonType::TypeParam(_) | AxonType::Union(_) => 0,
    }
}

/// CS2-3b: give each bare `Dict`'s unwritten value (`Dict(Unknown)`), and each
/// unwritten implicit arg of an applied struct (`App` arg `Unknown`), the type
/// `next` makes, left to right: a var a `let` or a fn signature owns, or a
/// struct's implicit param. With `structs` (a fn signature), a bare struct name
/// whose only params are implicit (`type Set = { d: Dict }`) becomes `App` too.
fn fill_bare_dicts(
    ty: Type,
    structs: Option<&HashMap<String, usize>>,
    next: &mut dyn FnMut() -> Type,
) -> Type {
    let mut rec = |t: Box<Type>| Box::new(fill_bare_dicts(*t, structs, next));
    match ty {
        Type::Dict(v) if *v == Type::Unknown => Type::Dict(Box::new(next())),
        Type::Dict(v) => Type::Dict(rec(v)),
        Type::Struct(n) => match structs.and_then(|s| s.get(&n)) {
            Some(&k) => Type::App(n, (0..k).map(|_| next()).collect()),
            None => Type::Struct(n),
        },
        Type::Option(i) => Type::Option(rec(i)),
        Type::Slice(i) => Type::Slice(rec(i)),
        Type::Chan(i) => Type::Chan(rec(i)),
        Type::Uncertain(i) => Type::Uncertain(rec(i)),
        Type::Temporal(i) => Type::Temporal(rec(i)),
        Type::RawPtr(i) => Type::RawPtr(rec(i)),
        Type::Result(o, e) => {
            let o = rec(o);
            Type::Result(o, rec(e))
        }
        Type::Tuple(ts) => Type::Tuple(
            ts.into_iter()
                .map(|t| fill_bare_dicts(t, structs, next))
                .collect(),
        ),
        Type::App(n, ts) => Type::App(
            n,
            ts.into_iter()
                .map(|t| match t {
                    Type::Unknown => next(),
                    t => fill_bare_dicts(t, structs, next),
                })
                .collect(),
        ),
        Type::Fn(ps, r) => {
            let ps = ps
                .into_iter()
                .map(|t| fill_bare_dicts(t, structs, next))
                .collect();
            Type::Fn(ps, Box::new(fill_bare_dicts(*r, structs, next)))
        }
        other => other,
    }
}

/// CS2-3b: the type variables left in `ty`, each once, in order.
fn collect_vars(ty: &Type, out: &mut Vec<u32>) {
    ty.clone().map_leaves(&mut |l| {
        if let Type::Var(v) = l {
            if !out.contains(&v) {
                out.push(v);
            }
        }
        l
    });
}

/// CS2-3b: `fns` grouped into their call graph's strongly connected
/// components, callees before callers (Tarjan emits them in that order);
/// `order` (source order) breaks ties. An edge is any mention of another
/// fn's name in a body: a call, a fn value, a method call by that name.
fn callee_first_groups<'a>(
    order: &[&'a str],
    fns: &HashMap<&'a str, &'a FnDef>,
) -> Vec<Vec<&'a str>> {
    let edges: HashMap<&str, Vec<&'a str>> = order
        .iter()
        .map(|&name| {
            let mut out: Vec<&'a str> = Vec::new();
            crate::ast::walk_expr(&fns[name].body, &mut |e| {
                let callee = match e {
                    Expr::Ident(n) => Some(n.as_str()),
                    Expr::MethodCall { method, .. } => Some(method.as_str()),
                    _ => None,
                };
                if let Some((&k, _)) = callee.and_then(|c| fns.get_key_value(c)) {
                    if !out.contains(&k) {
                        out.push(k);
                    }
                }
            });
            (name, out)
        })
        .collect();
    struct Tarjan<'a, 'e> {
        edges: &'e HashMap<&'a str, Vec<&'a str>>,
        index: HashMap<&'a str, usize>,
        low: HashMap<&'a str, usize>,
        stack: Vec<&'a str>,
        groups: Vec<Vec<&'a str>>,
    }
    impl<'a> Tarjan<'a, '_> {
        fn visit(&mut self, v: &'a str) {
            let i = self.index.len();
            self.index.insert(v, i);
            self.low.insert(v, i);
            self.stack.push(v);
            let edges = self.edges;
            for &w in &edges[v] {
                if !self.index.contains_key(w) {
                    self.visit(w);
                    let lw = self.low[w];
                    let lv = self.low.get_mut(v).unwrap();
                    *lv = (*lv).min(lw);
                } else if self.stack.contains(&w) {
                    let iw = self.index[w];
                    let lv = self.low.get_mut(v).unwrap();
                    *lv = (*lv).min(iw);
                }
            }
            if self.low[v] == self.index[v] {
                let at = self.stack.iter().rposition(|&x| x == v).unwrap();
                self.groups.push(self.stack.split_off(at));
            }
        }
    }
    let mut t = Tarjan {
        edges: &edges,
        index: HashMap::new(),
        low: HashMap::new(),
        stack: Vec::new(),
        groups: Vec::new(),
    };
    for &v in order {
        if !t.index.contains_key(v) {
            t.visit(v);
        }
    }
    t.groups
}

/// CS2-5: in a generic fn's signature, the fn's own type params named as a
/// generic struct's arguments (`B<T>`, resolved as `App("B", [Deferred("T")])`)
/// become `TypeParam`, so each call instantiates them. Elsewhere the sig keeps
/// its existing resolution.
fn generalize_app_args(ty: Type, generics: &HashSet<&str>) -> Type {
    let rec = |t: Box<Type>| Box::new(generalize_app_args(*t, generics));
    match ty {
        Type::App(n, args) => Type::App(
            n,
            args.into_iter()
                .map(|a| {
                    a.map_leaves(&mut |t| match t {
                        Type::Deferred(p) if generics.contains(p.as_str()) => Type::TypeParam(p),
                        t => t,
                    })
                })
                .collect(),
        ),
        Type::Option(i) => Type::Option(rec(i)),
        // CS2-3b: `fn f<V>(d: Dict<str, V>)` - the value is the fn's `V`.
        Type::Dict(v) => Type::Dict(Box::new(v.map_leaves(&mut |t| match t {
            Type::Deferred(p) if generics.contains(p.as_str()) => Type::TypeParam(p),
            t => t,
        }))),
        Type::Result(o, e) => Type::Result(rec(o), rec(e)),
        Type::Slice(i) => Type::Slice(rec(i)),
        Type::Tuple(ts) => Type::Tuple(
            ts.into_iter()
                .map(|t| generalize_app_args(t, generics))
                .collect(),
        ),
        Type::Fn(ps, r) => Type::Fn(
            ps.into_iter()
                .map(|t| generalize_app_args(t, generics))
                .collect(),
            rec(r),
        ),
        other => other,
    }
}

/// CS2-6: true when `ty` has a part not yet resolved to a concrete type.
fn has_unresolved(ty: &Type) -> bool {
    let mut found = false;
    ty.clone().map_leaves(&mut |t| {
        found |= type_is_unresolved(&t);
        t
    });
    found
}

/// CS2-6: true when `param` and `arg` are certainly different types: two
/// scalars of different kinds (number / `str` / `bool` / unit), compared
/// through `[T]`, `Option`, `Result` and same-length tuples. Any other shape,
/// and any number against any number, is left to the run time: a call
/// through a fn value is never refused for a type the call might still run.
fn kinds_conflict(param: &Type, arg: &Type) -> bool {
    fn kind(t: &Type) -> Option<u8> {
        match t {
            t if t.is_numeric() => Some(0),
            Type::Str => Some(1),
            Type::Bool => Some(2),
            Type::Unit => Some(3),
            _ => None,
        }
    }
    match (param, arg) {
        (Type::Slice(p), Type::Slice(a)) | (Type::Option(p), Type::Option(a)) => {
            kinds_conflict(p, a)
        }
        (Type::Result(po, pe), Type::Result(ao, ae)) => {
            kinds_conflict(po, ao) || kinds_conflict(pe, ae)
        }
        (Type::Tuple(ps), Type::Tuple(as_)) if ps.len() == as_.len() => {
            ps.iter().zip(as_).any(|(p, a)| kinds_conflict(p, a))
        }
        _ => matches!((kind(param), kind(arg)), (Some(p), Some(a)) if p != a),
    }
}

// ── Inference error ───────────────────────────────────────────────────────────

/// A diagnostic produced by the type-inference pass.
#[derive(Debug, Clone)]
pub struct InferError {
    pub code: &'static str,
    pub message: String,
    pub expected: Option<String>,
    pub found: Option<String>,
    pub span: crate::span::Span,
    /// Repair advice, when this mismatch has a known shape.
    ///
    /// `InferError` had no help channel at all, so every E0102 it raised was
    /// STRUCTURALLY unable to carry advice — the CLI even said so at the
    /// conversion site ("InferError has no fix") and passed `None`. Measured
    /// across the five places a `Result` gets misused, three carry good help
    /// (binop E0301, argument E0306, return E0307) and the two that come from
    /// here — a struct field and an annotated `let` — carried none.
    pub help: Option<String>,
}

/// Advice for the specific mismatch where a `Result`/`Option` was used where
/// its INNER type was wanted — the shape `parse_int(s)` produces, and the most
/// common mismatch there is.
///
/// Deliberately narrow: it fires only when `found` is the wrapper of exactly
/// the `expected` type, so it cannot mislabel an unrelated mismatch. The text
/// matches what E0301/E0302 already say for the same mistake elsewhere, because
/// one mistake should not get two different explanations depending on where it
/// lands.
fn mismatch_help(origin: &str, expected: &Type, found: &Type) -> Option<String> {
    // A STRING used as an index. `d["a"]` is how every other language spells a
    // map lookup, and `xs["a"]` is the same slip on an array — both produced a
    // bare "type mismatch in slice index (expected i64), found str" with no
    // advice at all. Axon indexes arrays by `i64` and reaches into a dict with
    // free functions, and neither of those is guessable from the message.
    if origin == "slice index" && matches!(expected, Type::I64) && matches!(found, Type::Str) {
        return Some(
            "an index is an `i64` — Axon has no `d[\"key\"]` syntax. For a \
             string-keyed map use `dict_get(d, k)` and `dict_set(d, k, v)`; for \
             an array, index with a number"
                .to_string(),
        );
    }
    wrapper_misuse_help(expected, found)
}

fn wrapper_misuse_help(expected: &Type, found: &Type) -> Option<String> {
    let inner = expected.display();
    match found {
        Type::Result(ok, _) if ok.display() == inner => Some(format!(
            "match it: `match x {{ Ok(v) => …  Err(e) => … }}` — each arm \
             produces a `{inner}`. Inside a fn that itself returns a `Result`, \
             `parse_int(s)?` propagates the error instead"
        )),
        Type::Option(v) if v.display() == inner => Some(format!(
            "match it: `match x {{ Some(v) => …  None => … }}` — each arm \
             produces a `{inner}`"
        )),
        _ => None,
    }
}

/// Whether a function body never falls through to a value, because its tail
/// diverts control flow.
///
/// Conservative and syntactic, matching `checker::is_terminator`: a bare
/// `return` in tail position, or a block whose last statement is one. An `if`
/// whose every branch returns is NOT counted — that needs branch analysis, and
/// counting it wrongly would SUPPRESS a real "missing return value" error.
fn body_diverges(e: &crate::ast::Expr) -> bool {
    use crate::ast::Expr;
    match e {
        Expr::Return(_) => true,
        Expr::Block(stmts) => stmts.last().is_some_and(|s| body_diverges(&s.expr)),
        _ => false,
    }
}

impl InferError {
    fn new(code: &'static str, msg: impl Into<String>) -> Self {
        InferError {
            code,
            message: msg.into(),
            expected: None,
            found: None,
            span: crate::span::Span::dummy(),
            help: None,
        }
    }

    fn mismatch(origin: &str, expected: &Type, found: &Type) -> Self {
        InferError {
            code: E0102,
            message: format!("type mismatch in {origin}"),
            expected: Some(expected.display()),
            found: Some(found.display()),
            span: crate::span::Span::dummy(),
            help: mismatch_help(origin, expected, found),
        }
    }

    pub fn with_span(mut self, span: crate::span::Span) -> Self {
        self.span = span;
        self
    }
}

// ── Scope ─────────────────────────────────────────────────────────────────────

/// Lexical scope: a stack of frames, innermost last.
pub struct Scope {
    frames: Vec<HashMap<String, Type>>,
}

impl Scope {
    pub fn new() -> Self {
        Scope {
            frames: vec![HashMap::new()],
        }
    }

    /// Push a new inner scope frame.
    pub fn push(&mut self) {
        self.frames.push(HashMap::new());
    }

    /// Pop the innermost scope frame.
    pub fn pop(&mut self) {
        if self.frames.len() > 1 {
            self.frames.pop();
        }
    }

    /// Bind `name` to `ty` in the innermost frame.
    pub fn bind(&mut self, name: String, ty: Type) {
        if let Some(frame) = self.frames.last_mut() {
            frame.insert(name, ty);
        }
    }

    /// Look up `name`, walking from innermost to outermost frame.
    pub fn lookup(&self, name: &str) -> Option<&Type> {
        for frame in self.frames.iter().rev() {
            if let Some(ty) = frame.get(name) {
                return Some(ty);
            }
        }
        None
    }
}

impl Default for Scope {
    fn default() -> Self {
        Self::new()
    }
}

// ── Function signature ────────────────────────────────────────────────────────

/// Resolved function signature (params + return type).
#[derive(Debug, Clone)]
pub struct FnSig {
    pub params: Vec<Type>,
    pub ret: Type,
}

// ── Type-string parser (for builtins with compound types) ─────────────────────

/// Parse a source-level type string like `"Result<i64,str>"` into a `Type`.
fn parse_type_str(s: &str) -> Type {
    let s = s.trim();
    if let Some(ty) = Type::from_name(s) {
        return ty;
    }
    if let Some(inner) = strip_wrap(s, "Result<", ">") {
        if let Some(comma) = top_level_comma(inner) {
            let ok = parse_type_str(&inner[..comma]);
            let err = parse_type_str(&inner[comma + 1..]);
            return Type::Result(Box::new(ok), Box::new(err));
        }
    }
    if let Some(inner) = strip_wrap(s, "Option<", ">") {
        return Type::Option(Box::new(parse_type_str(inner)));
    }
    if let Some(inner) = strip_wrap(s, "[", "]") {
        return Type::Slice(Box::new(parse_type_str(inner)));
    }
    if let Some(inner) = strip_wrap(s, "Chan<", ">") {
        return Type::Chan(Box::new(parse_type_str(inner)));
    }
    if let Some(inner) = strip_wrap(s, "Uncertain<", ">") {
        return Type::Uncertain(Box::new(parse_type_str(inner)));
    }
    if let Some(inner) = strip_wrap(s, "Temporal<", ">") {
        return Type::Temporal(Box::new(parse_type_str(inner)));
    }
    // CS2-3a: a builtin's closure param `fn(P, ..) -> R`; `fn(P, ..)` with no
    // `->` is a unit return. The call site frees a unit return (the builtin
    // discards the closure's result, `dict_each`, `http_sse`), so only the
    // params are unified with the closure's.
    if let Some(rest) = s.strip_prefix("fn(") {
        let mut depth = 0usize;
        let close = rest.char_indices().find_map(|(i, c)| match c {
            '(' => {
                depth += 1;
                None
            }
            ')' if depth == 0 => Some(i),
            ')' => {
                depth -= 1;
                None
            }
            _ => None,
        });
        if let Some(close) = close {
            let inner = rest[..close].trim();
            let params = if inner.is_empty() {
                Vec::new()
            } else {
                split_top_level(inner)
                    .into_iter()
                    .map(parse_type_str)
                    .collect()
            };
            let ret = match rest[close + 1..].trim() {
                "" => Type::Unit,
                after => match after.strip_prefix("->") {
                    Some(r) => parse_type_str(r),
                    None => return Type::Deferred(s.to_string()),
                },
            };
            return Type::Fn(params, Box::new(ret));
        }
    }
    // Tuple types: `(T1, T2, …)`. Top-level commas split the elements.
    // Required for builtin sigs like `arr_partition -> ([T], [T])`;
    // without this the whole string lands as one Deferred name and the
    // destructure consumer sees an opaque non-tuple, breaking inference.
    if let Some(inner) = strip_wrap(s, "(", ")") {
        if top_level_comma(inner).is_some() {
            return Type::Tuple(
                split_top_level(inner)
                    .into_iter()
                    .map(parse_type_str)
                    .collect(),
            );
        }
    }
    // CS2-3b: a builtin's `Dict` holds the sig's `V`; `Dict<X>` names its value.
    if s == "Dict" {
        return Type::Dict(Box::new(Type::Deferred("V".to_string())));
    }
    if let Some(inner) = strip_wrap(s, "Dict<", ">") {
        return Type::Dict(Box::new(parse_type_str(inner)));
    }
    Type::Deferred(s.to_string())
}

/// Split a type list at its top-level commas (`[T], (str, V)` -> two parts).
fn split_top_level(inner: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, c) in inner.char_indices() {
        match c {
            '<' | '[' | '(' => depth += 1,
            '>' | ']' | ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                parts.push(&inner[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&inner[start..]);
    parts
}

/// CS2-3a: the sig placeholder for a numeric key fn's return (`fn(T) -> Num`
/// in `arr_max_by`, `arr_min_by`, `arr_sum_by_f64`). Each call instantiates it
/// to a fresh variable that `solve()` requires to be an int or a float: the
/// interpreter takes either (an i64 key is compared or added as f64).
const NUM_PARAM: &str = "Num";

/// Collect the distinct type-variable names a builtin signature mentions, in
/// first-appearance order. Empty when the signature is not generic.
///
/// A builtin sig's `T`/`U`/`V` is a TYPE VARIABLE, but `parse_type_str` has no
/// arm for one, so it lands as `Deferred(name)` — and `unify` passes `Deferred`
/// through against ANY partner without error. Left that way a generic slot is a
/// permanent wildcard: it absorbs every constraint that touches it instead of
/// being fixed by one and checked against the rest.
fn builtin_type_param_names(params: &[Type], ret: &Type) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for p in params {
        collect_type_var_names(p, &mut out);
    }
    collect_type_var_names(ret, &mut out);
    out
}

fn collect_type_var_names(t: &Type, out: &mut Vec<String>) {
    match t {
        Type::Deferred(n) if crate::checker::is_builtin_type_param_name(n) => {
            if !out.iter().any(|e| e == n) {
                out.push(n.clone());
            }
        }
        Type::Slice(i)
        | Type::Option(i)
        | Type::Chan(i)
        | Type::Uncertain(i)
        | Type::Temporal(i)
        | Type::Dict(i) => collect_type_var_names(i, out),
        Type::Result(a, b) => {
            collect_type_var_names(a, out);
            collect_type_var_names(b, out);
        }
        Type::Tuple(ts) => {
            for t in ts {
                collect_type_var_names(t, out);
            }
        }
        Type::Fn(ps, r) => {
            for p in ps {
                collect_type_var_names(p, out);
            }
            collect_type_var_names(r, out);
        }
        _ => {}
    }
}

/// Rewrite every `Deferred(n)` where `n` is one of `names` into `TypeParam(n)`,
/// structurally, through a closure param's `fn(..) -> ..` too (CS2-3a), so
/// its `T`/`U` are the same variables as the rest of the sig. Other
/// `Deferred`s (opaque types like `Dict`) are left alone.
fn promote_type_params(t: &Type, names: &[String]) -> Type {
    let rec = |i: &Type| Box::new(promote_type_params(i, names));
    match t {
        Type::Deferred(n) if names.iter().any(|e| e == n) => Type::TypeParam(n.clone()),
        Type::Slice(i) => Type::Slice(rec(i)),
        Type::Option(i) => Type::Option(rec(i)),
        Type::Chan(i) => Type::Chan(rec(i)),
        Type::Uncertain(i) => Type::Uncertain(rec(i)),
        Type::Temporal(i) => Type::Temporal(rec(i)),
        Type::Dict(i) => Type::Dict(rec(i)),
        Type::Result(a, b) => Type::Result(rec(a), rec(b)),
        Type::Tuple(ts) => Type::Tuple(ts.iter().map(|e| promote_type_params(e, names)).collect()),
        Type::Fn(ps, r) => Type::Fn(
            ps.iter().map(|p| promote_type_params(p, names)).collect(),
            rec(r),
        ),
        other => other.clone(),
    }
}

fn strip_wrap<'a>(s: &'a str, prefix: &str, suffix: &str) -> Option<&'a str> {
    s.strip_prefix(prefix)?.strip_suffix(suffix)
}

fn top_level_comma(s: &str) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '<' | '[' | '(' => depth += 1,
            '>' | ']' | ')' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => return Some(i),
            _ => {}
        }
    }
    None
}

/// CS2-6: a call through a fn value (an index, a call, a field, a lambda, or a
/// local), checked once the substitution is final. The callee's fn type is
/// often a variable at the call (`ops[0]`), so the check waits for `solve()`.
struct CallObligation {
    callee: Type,
    /// Each argument's type.
    args: Vec<Type>,
    /// The fresh variable the call expression was typed as.
    ret: Type,
    span: crate::span::Span,
}

// ── InferCtx ──────────────────────────────────────────────────────────────────

/// The type-inference context.
pub struct InferCtx {
    next_var: u32,
    constraints: Vec<Constraint>,
    /// Call-site signatures: builtins, plus every user fn that a plain call
    /// reaches. A user fn named like a builtin is NOT here — a plain call
    /// reaches the builtin (`builtins::call_prefers_builtin`, CS-4).
    pub fn_sigs: HashMap<String, FnSig>,
    /// Every top-level user fn's own signature, shadowing ones included: the
    /// call-site sig when `call_prefers_builtin` is false (a `&mut` call), and
    /// the fn's own param types for borrow checking and the LSP.
    pub user_fn_sigs: HashMap<String, FnSig>,
    /// Type parameter names for generic user functions (fn_name → [param_names]).
    pub generic_fn_params: HashMap<String, Vec<String>>,
    /// Type-variable slot names of each generic BUILTIN (`T`, `V`, ...), kept
    /// apart from `generic_fn_params`: a user fn may share a builtin's name
    /// (CS-4), and the names a call instantiates must come from the sig that
    /// call actually reaches.
    builtin_generic_params: HashMap<String, Vec<String>>,
    /// Recorded call-site instantiations (fn_name → Vec<concrete Type args>).
    /// Populated during infer; consumed by the mono pass.
    pub call_instantiations: Vec<(String, Vec<Type>)>,
    /// CS-5: `len` args that were still a type variable at the call. Each
    /// must resolve to `str` or `[T]`; checked by `solve()` once the
    /// substitution is final (pinning the var to `str` at the call refused a
    /// `?`- or match-bound array).
    len_obligations: Vec<(Type, crate::span::Span)>,
    /// CS2-3a: the `Num` variable of each call to a numeric-key builtin, which
    /// `solve()` requires to be an int or a float.
    num_obligations: Vec<(Type, crate::span::Span)>,
    /// CS2-6: calls whose callee is not a fn name with a signature. Checked
    /// by `solve()` against the callee's resolved fn type.
    call_obligations: Vec<CallObligation>,
    /// Field lists for user-defined struct types.
    pub struct_fields: HashMap<String, Vec<(String, Type)>>,
    /// Generic type parameter names for struct types (struct_name → [param_names]).
    pub struct_generic_params: HashMap<String, Vec<String>>,
    /// CS2-3b: how many of a struct's `struct_generic_params` are declared
    /// (`type B<T>`); the rest are implicit, one per bare `Dict` in a field.
    struct_declared_params: HashMap<String, usize>,
    /// CS2-3b: a top-level fn's value vars for the bare `Dict`s in its
    /// signature (params, then return), in order. Solved from the fn's body.
    dict_owned_vars: HashMap<String, Vec<u32>>,
    /// CS2-3b: the implicit type params of a fn whose bare-`Dict` value the
    /// body left free; each call instantiates them like declared ones.
    implicit_fn_params: HashMap<String, Vec<String>>,
    /// CS2-3b: the constraints of the bare-`Dict` fns' bodies, unified before
    /// the rest (`solve_pending`); `solve` starts from it.
    partial: Substitution,
    /// CS2-5: each method call whose receiver was still a type variable when
    /// inferred (an untyped lambda param), keyed by the receiver's address.
    method_receiver_vars: Vec<(usize, Type)>,
    /// CS2-5: the receivers of `method_receiver_vars` that resolved to a
    /// declared type parameter, with its name. The checker looks the method
    /// up in that parameter's bounds, as for a receiver declared `x: T`.
    pub method_receiver_type_params: HashMap<usize, String>,
    /// Variant name lists for user-defined enum types.
    pub enum_variants: HashMap<String, Vec<String>>,
    /// Phase 5: named refinement types → their (erased) base AxonType. A
    /// `type Positive = i64 where _ > 0` resolves `Positive` to `i64` — the
    /// refinement is transparent at the value/layout level (the predicate is a
    /// static proof obligation, not a runtime representation change).
    pub refinement_base: HashMap<String, AxonType>,
    /// Fix #2: payload field types per variant.
    /// Maps `enum_name → variant_name → [(field_name, field_type)]`.
    pub enum_variant_fields: HashMap<String, HashMap<String, Vec<(String, Type)>>>,
    /// Phase 3: trait method signatures for `dyn Trait` method call type inference.
    /// Maps `trait_name → method_name → FnSig` (self excluded from params).
    pub trait_method_sigs: HashMap<String, HashMap<String, FnSig>>,
    /// Module-level `let` binding types (e.g. `let KB = comptime { 1024 }`).
    /// Populated during infer_program for Item::LetDef.
    pub module_bindings: HashMap<String, Type>,
    /// Errors accumulated during inference and unification.
    pub errors: Vec<InferError>,
    pub file: String,
    /// Span of the statement currently being inferred; used to tag errors.
    current_stmt_span: crate::span::Span,
    /// Deepest type a binding may have (`None`: unbounded). See [`MAX_TYPE_DEPTH`].
    max_type_depth: Option<usize>,
    /// Set only inside [`InferCtx::infer_predicate`]: each integer literal
    /// that is an operand of an arithmetic or comparison `BinOp`, with the
    /// other operand's (unsolved) type and the span (CS-1/CS-2, E-6).
    predicate_int_operands: Option<Vec<(i64, Type, crate::span::Span)>>,
}

/// What [`InferCtx::infer_predicate`] found in one predicate.
pub struct PredicateTyping {
    /// The predicate's solved type (the caller decides whether it is `bool`).
    pub ty: Type,
    /// Integer literals compared with (or combined with) an `f64`/`f32`
    /// operand: the runtime does not widen these, so they panic there.
    pub int_against_float: Vec<(i64, Type, crate::span::Span)>,
    /// Every other type error inside the predicate.
    pub errors: Vec<InferError>,
}

/// Extract a simple string name from an `AxonType` for impl-method name mangling.
fn ast_type_simple_name(ty: &AxonType) -> String {
    match ty {
        AxonType::Named(n) => n.clone(),
        AxonType::Generic { base, .. } => base.clone(),
        _ => "Unknown".into(),
    }
}

impl InferCtx {
    /// AX-63: a binding whose type nests past [`MAX_TYPE_DEPTH`] is refused here,
    /// at the binding, and bound as `Unknown` so the lines after it stay shallow
    /// and report nothing further.
    fn refuse_deep_binding(&mut self, name: &str, ty: Type) -> Type {
        let Some(limit) = self.max_type_depth else {
            return ty;
        };
        if !ty.nests_deeper_than(limit) {
            return ty;
        }
        let span = self.current_stmt_span;
        let mut err = InferError::new(
            E0301,
            format!(
                "the type of `{name}` is nested more than {limit} levels deep (the wasm32 limit)"
            ),
        )
        .with_span(span);
        err.help = Some(format!(
            "on wasm32 a type may nest at most {limit} levels (`[[[…i64…]]]`, `Some(Some(…))`); \
             build the value in a loop or wrap it in a struct, or run on a native build"
        ));
        self.errors.push(err);
        Type::Unknown
    }

    /// Create a fresh context and pre-register all built-in signatures.
    pub fn new(file: impl Into<String>) -> Self {
        let mut ctx = InferCtx {
            next_var: 0,
            constraints: Vec::new(),
            fn_sigs: HashMap::new(),
            user_fn_sigs: HashMap::new(),
            generic_fn_params: HashMap::new(),
            builtin_generic_params: HashMap::new(),
            call_instantiations: Vec::new(),
            len_obligations: Vec::new(),
            num_obligations: Vec::new(),
            call_obligations: Vec::new(),
            struct_fields: HashMap::new(),
            struct_generic_params: HashMap::new(),
            struct_declared_params: HashMap::new(),
            dict_owned_vars: HashMap::new(),
            implicit_fn_params: HashMap::new(),
            partial: Substitution::new(),
            method_receiver_vars: Vec::new(),
            method_receiver_type_params: HashMap::new(),
            enum_variants: HashMap::new(),
            refinement_base: HashMap::new(),
            enum_variant_fields: HashMap::new(),
            trait_method_sigs: HashMap::new(),
            module_bindings: HashMap::new(),
            errors: Vec::new(),
            file: file.into(),
            current_stmt_span: crate::span::Span::dummy(),
            max_type_depth: MAX_TYPE_DEPTH,
            predicate_int_operands: None,
        };
        for (name, sig) in builtin_sigs() {
            let params: Vec<Type> = sig.params.iter().map(|s| parse_type_str(s)).collect();
            let ret = parse_type_str(&sig.ret);
            // Promote type-variable slots to `TypeParam` and register the builtin
            // as generic, so each CALL SITE instantiates fresh `Var`s via
            // `instantiate_sig` — vars that unify once and then stay bound, rather
            // than `Deferred` wildcards that silently swallow a mismatch.
            let tp_names = builtin_type_param_names(&params, &ret);
            let (params, ret) = if tp_names.is_empty() {
                (params, ret)
            } else {
                (
                    params
                        .iter()
                        .map(|t| promote_type_params(t, &tp_names))
                        .collect(),
                    promote_type_params(&ret, &tp_names),
                )
            };
            if !tp_names.is_empty() {
                ctx.builtin_generic_params.insert(name.clone(), tp_names);
            }
            ctx.fn_sigs.insert(name, FnSig { params, ret });
        }
        ctx
    }

    /// Allocate a fresh unification variable.
    pub fn fresh(&mut self) -> Type {
        let n = self.next_var;
        self.next_var += 1;
        Type::Var(n)
    }

    /// Register a top-level user fn's signature: always in `user_fn_sigs`,
    /// and in the call-site table `fn_sigs` unless a plain call by that name
    /// reaches the builtin instead (CS-4).
    fn insert_user_fn_sig(&mut self, name: &str, sig: FnSig) {
        if !crate::builtins::call_prefers_builtin(name, false) {
            self.fn_sigs.insert(name.to_string(), sig.clone());
        }
        self.user_fn_sigs.insert(name.to_string(), sig);
    }

    /// CS2-3b: the structs whose type params are all implicit (one per bare
    /// `Dict` field, none declared), with how many there are.
    fn implicit_only_structs(&self) -> HashMap<String, usize> {
        self.struct_declared_params
            .iter()
            .filter(|(_, &declared)| declared == 0)
            .filter_map(|(n, _)| Some((n.clone(), self.struct_generic_params.get(n)?.len())))
            .collect()
    }

    /// CS2-3b: `Dict<K, V>` with `K` other than `str` is refused: a dict's
    /// keys are always strings (`dict_set` takes `k: str`).
    fn check_dict_keys(&mut self, ty: &AxonType, span: crate::span::Span) {
        let mut bad: Vec<String> = Vec::new();
        fn walk(t: &AxonType, bad: &mut Vec<String>) {
            match t {
                AxonType::Generic { base, args } => {
                    if base == "Dict" && args.len() == 2 {
                        if !matches!(&args[0], AxonType::Named(n) if n == "str" || n == "String") {
                            bad.push(format!("{:?}", args[0]));
                        }
                    }
                    args.iter().for_each(|a| walk(a, bad));
                }
                AxonType::Result { ok, err } => {
                    walk(ok, bad);
                    walk(err, bad);
                }
                AxonType::Option(i)
                | AxonType::Chan(i)
                | AxonType::Slice(i)
                | AxonType::Ref(i)
                | AxonType::RefMut(i)
                | AxonType::RawPtr(i) => walk(i, bad),
                AxonType::Tuple(ts) => ts.iter().for_each(|a| walk(a, bad)),
                AxonType::Fn { params, ret } => {
                    params.iter().for_each(|a| walk(a, bad));
                    walk(ret, bad);
                }
                _ => {}
            }
        }
        walk(ty, &mut bad);
        for _ in bad {
            self.errors.push(
                InferError::new(E0102, "Dict keys are str: write `Dict<str, V>`").with_span(span),
            );
        }
    }

    /// CS2-3b: unify the constraints gathered so far into `self.partial`, so
    /// a fn's own bare-`Dict` vars are known before its callers are inferred.
    fn solve_pending(&mut self) {
        let mut subst = std::mem::take(&mut self.partial);
        for c in std::mem::take(&mut self.constraints) {
            self.current_stmt_span = c.span;
            self.unify(c.lhs, c.rhs, &c.origin, &mut subst);
        }
        self.partial = subst;
    }

    /// CS2-3b: after `fns`' bodies are solved, a bare-`Dict` var a body fixed
    /// keeps its type (callers must agree); one the body left free becomes an
    /// implicit type param of the fn, instantiated fresh per call. One the
    /// body bound to a declared `T` (which a body sees as `Deferred("T")`)
    /// reads `Dict(T)` in the signature, so each call instantiates it with
    /// that call's `T`: `put<T>(d: Dict, v: T) { dict_set(d, "x", v) }` no
    /// longer lets a caller write a `str` into an `i64` dict.
    fn generalize_dict_vars(&mut self, fns: &[&str]) {
        for &name in fns {
            let Some(owned) = self.dict_owned_vars.get(name) else {
                continue;
            };
            let mut free: Vec<u32> = Vec::new();
            for &v in owned {
                collect_vars(&self.partial.apply(&Type::Var(v)), &mut free);
            }
            let declared = self.generic_fn_params.get(name).cloned().unwrap_or_default();
            let partial = &self.partial;
            let generalize = |t: &Type| {
                partial.apply(t).map_leaves(&mut |l| match l {
                    Type::Var(w) if free.contains(&w) => Type::TypeParam(format!("__dv{w}")),
                    Type::Deferred(n) if declared.contains(&n) => Type::TypeParam(n),
                    l => l,
                })
            };
            let sig = match self.user_fn_sigs.get(name) {
                Some(s) => FnSig {
                    params: s.params.iter().map(&generalize).collect(),
                    ret: generalize(&s.ret),
                },
                None => continue,
            };
            self.insert_user_fn_sig(name, sig);
            if !free.is_empty() {
                let names: Vec<String> = free.iter().map(|w| format!("__dv{w}")).collect();
                self.implicit_fn_params.insert(name.to_string(), names);
            }
        }
    }

    /// CS2-3b: `name`'s signature as a value or a method: the implicit type
    /// params of a generalized bare-`Dict` fn instantiated fresh.
    fn sig_instance(&mut self, name: &str) -> Option<FnSig> {
        let sig = self.fn_sigs.get(name)?.clone();
        match self.implicit_fn_params.get(name).cloned() {
            Some(names) => Some(self.instantiate_sig(&sig, &names).0),
            None => Some(sig),
        }
    }

    /// Record that `lhs` and `rhs` must unify.
    pub fn constrain(&mut self, lhs: Type, rhs: Type, origin: &str) {
        let span = self.current_stmt_span;
        self.constraints.push(Constraint {
            lhs,
            rhs,
            origin: origin.to_string(),
            span,
        });
    }

    /// R19 Slice A: if `value` is an integer literal and `target` is a
    /// fixed-width/unsigned integer type differing from the literal's `i64`
    /// default, range-check it (E1900) and return `true` — the literal coerces
    /// to `target`, so the caller skips the normal width-mismatch constraint.
    /// Sound by construction: only *literals* coerce; a non-literal int still
    /// needs an explicit `as` cast, and unsigned arithmetic stays E0102-rejected
    /// until Slice B adds width-correct ops (no i64-backed half-measure, I-9).
    fn try_int_literal_coercion(&mut self, value: &Expr, val_ty: &Type, target: &Type) -> bool {
        if let Expr::Literal(AstLiteral::Int(n)) = value {
            if is_int_type(target) && is_int_type(val_ty) && target != val_ty {
                if !int_lit_in_range(*n, target) {
                    let span = self.current_stmt_span;
                    self.errors.push(
                        InferError::new(
                            E1900,
                            format!("literal {n} out of range for {}", target.display()),
                        )
                        .with_span(span),
                    );
                }
                return true;
            }
        }
        false
    }

    /// Point every refinement at the end of its chain, once (each link is
    /// visited once in total), so [`Self::resolve_ast_type`] takes one hop per
    /// name instead of re-walking the chain on every call: resolving each of
    /// `N` chained refinements (CS-1 types every refinement predicate's `_`)
    /// was `N²` hops, and a 50,000-link chain timed out the debug wasm check.
    /// The end is where the walk in `resolve_ast_type` stops: a builtin name,
    /// a name that is not a refinement, or a non-name type. A chain that runs
    /// into a cycle keeps its entries, so it still resolves to `Deferred`
    /// exactly as the walk decides.
    fn flatten_refinement_chains(&mut self) {
        // `None`: the chain runs into a cycle; leave it alone.
        let mut end: HashMap<String, Option<AxonType>> =
            HashMap::with_capacity(self.refinement_base.len());
        for start in self.refinement_base.keys() {
            if end.contains_key(start) {
                continue;
            }
            let mut path: Vec<&str> = Vec::new();
            let mut on_path: HashSet<&str> = HashSet::new();
            let mut cur = start.as_str();
            let terminal = loop {
                if let Some(t) = end.get(cur) {
                    break t.clone();
                }
                if !on_path.insert(cur) {
                    break None;
                }
                path.push(cur);
                match &self.refinement_base[cur] {
                    AxonType::Named(n)
                        if Type::from_name(n).is_none() && self.refinement_base.contains_key(n) =>
                    {
                        cur = n.as_str();
                    }
                    base => break Some(base.clone()),
                }
            };
            for p in path {
                end.insert(p.to_string(), terminal.clone());
            }
        }
        for (name, t) in end {
            if let Some(t) = t {
                self.refinement_base.insert(name, t);
            }
        }
    }

    /// Convert an AST `AxonType` to a semantic `Type`.
    pub fn resolve_ast_type(&self, ast_ty: &AxonType) -> Type {
        // Phase 5: a named refinement is transparent — resolve to its base type.
        // A chain of refinements (`type A1 = A0 where …`, `type A2 = A1 where …`)
        // is followed with a loop, not one frame per link (AX-66: `N` links was
        // `N` nested calls, a wasm32 trap at 5,000 on flat source). A name seen
        // twice on the chase is a cycle (`type A = B …`, `type B = A …`), which
        // used to overflow the stack; it resolves like any other name nothing
        // defines (`Deferred`).
        let mut ast_ty = ast_ty;
        let mut chased: Option<HashSet<&str>> = None;
        while let AxonType::Named(name) = ast_ty {
            if Type::from_name(name).is_some() {
                break;
            }
            let Some(base) = self.refinement_base.get(name) else {
                break;
            };
            if !chased
                .get_or_insert_with(HashSet::new)
                .insert(name.as_str())
            {
                return Type::Deferred(name.clone());
            }
            ast_ty = base;
        }
        match ast_ty {
            AxonType::Named(name) => {
                if let Some(ty) = Type::from_name(name) {
                    ty
                } else if self.struct_fields.contains_key(name) {
                    Type::Struct(name.clone())
                } else if self.enum_variants.contains_key(name) {
                    Type::Enum(name.clone())
                } else if name == "Dict" {
                    // CS2-3b: a bare `Dict`. `Unknown` marks its value as not
                    // written; `fill_bare_dicts` gives it a var where the
                    // context owns one (a `let`, a fn signature, a field).
                    Type::Dict(Box::new(Type::Unknown))
                } else {
                    Type::Deferred(name.clone())
                }
            }
            AxonType::Result { ok, err } => Type::Result(
                Box::new(self.resolve_ast_type(ok)),
                Box::new(self.resolve_ast_type(err)),
            ),
            AxonType::Option(inner) => Type::Option(Box::new(self.resolve_ast_type(inner))),
            AxonType::Slice(inner) => Type::Slice(Box::new(self.resolve_ast_type(inner))),
            AxonType::Chan(inner) => Type::Chan(Box::new(self.resolve_ast_type(inner))),
            AxonType::Generic { base, args } => {
                // Uncertain<T> and Temporal<T> are first-class in the type system.
                if base == "Uncertain" {
                    let inner = args
                        .first()
                        .map(|a| self.resolve_ast_type(a))
                        .unwrap_or(Type::Unknown);
                    return Type::Uncertain(Box::new(inner));
                }
                if base == "Temporal" {
                    let inner = args
                        .first()
                        .map(|a| self.resolve_ast_type(a))
                        .unwrap_or(Type::Unknown);
                    return Type::Temporal(Box::new(inner));
                }
                // CS2-3b: `Dict<str, V>` is `Dict(V)`. Keys are `str`
                // (`check_dict_keys` refuses another key type).
                if base == "Dict" && !self.struct_fields.contains_key(base) {
                    let value = match args.as_slice() {
                        [_, v] | [v] => self.resolve_ast_type(v),
                        _ => Type::Unknown,
                    };
                    return Type::Dict(Box::new(value));
                }
                // CS2-5: a user generic struct with its full argument list is
                // `App`, so `B<i64>` and `B<str>` are different types. CS2-3b:
                // the struct's implicit params (its bare-`Dict` fields) follow
                // the declared ones, unwritten (`Unknown`).
                if let Some(ps) = self.struct_generic_params.get(base) {
                    let declared = self
                        .struct_declared_params
                        .get(base)
                        .copied()
                        .unwrap_or(ps.len());
                    if declared == args.len() && declared > 0 {
                        let mut resolved: Vec<Type> =
                            args.iter().map(|a| self.resolve_ast_type(a)).collect();
                        resolved.resize(ps.len(), Type::Unknown);
                        return Type::App(base.clone(), resolved);
                    }
                }
                // Phase 1: other generics treated as deferred.
                Type::Deferred(base.clone())
            }
            AxonType::Fn { params, ret } => Type::Fn(
                params.iter().map(|p| self.resolve_ast_type(p)).collect(),
                Box::new(self.resolve_ast_type(ret)),
            ),
            // Phase 1: ref transparent (`&mut` too — its mode is checked by `mut_borrow`)
            AxonType::Ref(inner) | AxonType::RefMut(inner) => self.resolve_ast_type(inner),
            AxonType::TypeParam(name) => Type::TypeParam(name.clone()),
            AxonType::DynTrait(name) => Type::DynTrait(name.clone()),
            AxonType::Tuple(elems) => {
                Type::Tuple(elems.iter().map(|e| self.resolve_ast_type(e)).collect())
            }
            // Union types are not yet first-class in the semantic type system.
            // Treat them permissively: resolve to `Type::Unknown` so unification
            // does not assert a specific element type.
            AxonType::Union(_) => Type::Unknown,
            // R17 HAL: raw pointer type — substrate-only.
            AxonType::RawPtr(inner) => Type::RawPtr(Box::new(self.resolve_ast_type(inner))),
        }
    }

    // ── First pass: collect top-level signatures ──────────────────────────────

    /// Register all top-level `fn`, `type`, and `enum` declarations so that
    /// forward references resolve correctly during body inference.
    ///
    /// Uses a two-pass approach (Fix #7):
    /// - Pass 1: register all type names (structs, enums, functions) without
    ///   resolving field types, so that forward references are known.
    /// - Pass 2: resolve all field types now that all names are registered.
    pub fn collect_sigs(&mut self, program: &Program) {
        // Pass 1: register all names without resolving field/variant types.
        for item in &program.items {
            match item {
                Item::FnDef(f) => {
                    // Register with placeholder types; pass 2 will resolve properly.
                    self.insert_user_fn_sig(
                        &f.name,
                        FnSig {
                            params: vec![],
                            ret: Type::Unit,
                        },
                    );
                }
                Item::TypeDef(td) => {
                    // Register struct name so forward refs resolve to Type::Struct.
                    self.struct_fields.insert(td.name.clone(), vec![]);
                    // CS2-3b: each bare `Dict` in a field is an implicit type
                    // param of the struct, after the declared ones, so a
                    // literal types its dict's value (`Type::App`).
                    let bare = td
                        .fields
                        .iter()
                        .map(|f| count_bare_dicts(&f.ty))
                        .sum::<usize>();
                    if !td.generic_params.is_empty() || bare > 0 {
                        let mut ps = td.generic_params.clone();
                        ps.extend((0..bare).map(|i| format!("__dict{i}")));
                        self.struct_generic_params.insert(td.name.clone(), ps);
                        self.struct_declared_params
                            .insert(td.name.clone(), td.generic_params.len());
                    }
                }
                Item::EnumDef(ed) => {
                    // Register enum name so forward refs resolve to Type::Enum.
                    let variants = ed.variants.iter().map(|v| v.name.clone()).collect();
                    self.enum_variants.insert(ed.name.clone(), variants);
                }
                Item::RefineDef(r) => {
                    // Phase 5: a named refinement resolves to its (erased) base.
                    self.refinement_base.insert(r.name.clone(), r.base.clone());
                }
                Item::ImplBlock(blk) => {
                    // Register impl methods as placeholder sigs (mangled name: TypeName__method).
                    let type_name = ast_type_simple_name(&blk.for_type);
                    for m in &blk.methods {
                        let mangled = format!("{type_name}__{}", m.name);
                        self.fn_sigs.insert(
                            mangled,
                            FnSig {
                                params: vec![],
                                ret: Type::Unit,
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        self.flatten_refinement_chains();

        // Pass 2a: struct fields. CS2-3b: a bare `Dict` field's value is its
        // implicit param, `Deferred("__dictN")` like a declared `T`.
        for item in &program.items {
            if let Item::TypeDef(td) = item {
                for f in &td.fields {
                    self.check_dict_keys(&f.ty, td.span);
                }
                let names: Vec<String> = self
                    .struct_generic_params
                    .get(&td.name)
                    .map(|ps| ps[td.generic_params.len()..].to_vec())
                    .unwrap_or_default();
                let mut next = names.into_iter();
                let fields = td
                    .fields
                    .iter()
                    .map(|f| {
                        let ty = self.resolve_ast_type(&f.ty);
                        let ty = fill_bare_dicts(ty, None, &mut || {
                            next.next().map(Type::Deferred).unwrap_or(Type::Unknown)
                        });
                        (f.name.clone(), ty)
                    })
                    .collect();
                self.struct_fields.insert(td.name.clone(), fields);
            }
        }

        // Pass 2: resolve all field types now that all names are known.
        for item in &program.items {
            match item {
                Item::FnDef(f) => {
                    // Generic params (e.g. T in fn foo<T>(x: T)) must resolve to
                    // TypeParam, not Deferred, so trait bound checks can identify them.
                    let gset: std::collections::HashSet<&str> =
                        f.generic_params.iter().map(|s| s.as_str()).collect();
                    let resolve_with_generics = |ty: &AxonType| -> Type {
                        if let AxonType::Named(n) = ty {
                            if gset.contains(n.as_str()) {
                                return Type::TypeParam(n.clone());
                            }
                        }
                        generalize_app_args(self.resolve_ast_type(ty), &gset)
                    };
                    let params: Vec<Type> = f
                        .params
                        .iter()
                        .map(|p| resolve_with_generics(&p.ty))
                        .collect();
                    let ret = f
                        .return_type
                        .as_ref()
                        .map(resolve_with_generics)
                        .unwrap_or(Type::Unit);
                    // CS2-3b: a bare `Dict` in the signature, and a bare struct
                    // name whose params are all implicit, holds a var the fn
                    // owns, solved from its body before its callers
                    // (`infer_program`).
                    for p in &f.params {
                        self.check_dict_keys(&p.ty, f.span);
                    }
                    if let Some(r) = &f.return_type {
                        self.check_dict_keys(r, f.span);
                    }
                    let structs = self.implicit_only_structs();
                    let mut owned: Vec<u32> = Vec::new();
                    let mut fresh = || {
                        let v = self.next_var;
                        self.next_var += 1;
                        owned.push(v);
                        Type::Var(v)
                    };
                    let params: Vec<Type> = params
                        .into_iter()
                        .map(|t| fill_bare_dicts(t, Some(&structs), &mut fresh))
                        .collect();
                    let ret = fill_bare_dicts(ret, Some(&structs), &mut fresh);
                    if !owned.is_empty() {
                        self.dict_owned_vars.insert(f.name.clone(), owned);
                    }
                    self.insert_user_fn_sig(&f.name, FnSig { params, ret });
                    if !f.generic_params.is_empty() {
                        self.generic_fn_params
                            .insert(f.name.clone(), f.generic_params.clone());
                    }
                }
                Item::EnumDef(ed) => {
                    // Fix #2: store payload field types for each variant now
                    // that all type names are known from pass 1.
                    let mut variant_map: HashMap<String, Vec<(String, Type)>> = HashMap::new();
                    for variant in &ed.variants {
                        let fields: Vec<(String, Type)> = variant
                            .fields
                            .iter()
                            .map(|f| (f.name.clone(), self.resolve_ast_type(&f.ty)))
                            .collect();
                        variant_map.insert(variant.name.clone(), fields);
                    }
                    self.enum_variant_fields
                        .insert(ed.name.clone(), variant_map);
                }
                Item::ImplBlock(blk) => {
                    // Pass 2: register impl methods with fully-resolved types.
                    let type_name = ast_type_simple_name(&blk.for_type);
                    for m in &blk.methods {
                        let mangled = format!("{type_name}__{}", m.name);
                        let params = m
                            .params
                            .iter()
                            .map(|p| self.resolve_ast_type(&p.ty))
                            .collect();
                        let ret = m
                            .return_type
                            .as_ref()
                            .map(|t| self.resolve_ast_type(t))
                            .unwrap_or(Type::Unit);
                        self.fn_sigs.insert(mangled, FnSig { params, ret });
                    }
                }
                Item::TraitDef(td) => {
                    // Pass 2: register trait method sigs for `dyn Trait` method call inference.
                    let mut method_sigs = HashMap::new();
                    for m in &td.methods {
                        // Exclude the `self` param; callers are the non-self args.
                        let params: Vec<Type> = m
                            .params
                            .iter()
                            .filter(|p| p.name != "self")
                            .map(|p| self.resolve_ast_type(&p.ty))
                            .collect();
                        let ret = m
                            .return_type
                            .as_ref()
                            .map(|t| self.resolve_ast_type(t))
                            .unwrap_or(Type::Unit);
                        method_sigs.insert(m.name.clone(), FnSig { params, ret });
                    }
                    self.trait_method_sigs.insert(td.name.clone(), method_sigs);
                }
                _ => {}
            }
        }
    }

    // ── Expression inference ──────────────────────────────────────────────────

    /// A `BinOp` node and everything below it on the left, inferred without
    /// recursing down the left spine (AX-64): `1 + 1 + … + 1` is `N` nodes deep
    /// to the left, and one `infer_expr` frame per level overflows the stack
    /// at 100,000 terms. The spine is walked down once and inferred innermost
    /// first, the order the recursion visited it in (left operand, right
    /// operand, then the node's own constraints).
    fn infer_binop_chain(&mut self, expr: &Expr, scope: &mut Scope, ret_ty: &Type) -> Type {
        let mut spine: Vec<(&BinOp, &Expr, &Expr)> = Vec::new();
        let mut cur = expr;
        while let Expr::BinOp { op, left, right } = cur {
            spine.push((op, left, right));
            cur = left;
        }
        // `cur` is the innermost left operand: not a `BinOp`.
        let mut lt = self.infer_expr(cur, scope, ret_ty);
        for (op, left, right) in spine.into_iter().rev() {
            let rt = self.infer_expr(right, scope, ret_ty);
            lt = self.infer_binop_node(op, left, right, lt, rt);
        }
        lt
    }

    /// One `BinOp` node's own constraints and result type, given its operands'.
    fn infer_binop_node(
        &mut self,
        op: &BinOp,
        left: &Expr,
        right: &Expr,
        lt: Type,
        rt: Type,
    ) -> Type {
        // CS-1/CS-2 (E-6): in a predicate, note each integer-literal operand
        // of an arithmetic or comparison op with the other operand's type, so
        // `infer_predicate` can refuse it against an f64 once solved.
        if !matches!(
            op,
            BinOp::And
                | BinOp::Or
                | BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr
        ) {
            let span = self.current_stmt_span;
            if let Some(seen) = self.predicate_int_operands.as_mut() {
                for (lit, other) in [(left, &rt), (right, &lt)] {
                    if let Expr::Literal(AstLiteral::Int(n)) = lit {
                        seen.push((*n, other.clone(), span));
                    }
                }
            }
        }
        // Layer-2 ASI propagation: if either operand is `Uncertain<T>`,
        // the result is Uncertain. The bare T is treated as having
        // confidence 1.0 conceptually; soft unification in `unify`
        // already accepts Uncertain<T> on either side of T constraints.
        let is_unc = |t: &Type| matches!(t, Type::Uncertain(_));
        let unc_inner = |t: &Type| -> Option<Type> {
            match t {
                Type::Uncertain(inner) => Some(*inner.clone()),
                _ => None,
            }
        };
        // R19 Slice B helpers for the BinOp arm.
        // True when the type is a non-i64 fixed-width integer (signed or unsigned).
        let is_sized_int = |t: &Type| {
            matches!(
                t,
                Type::I8 | Type::I16 | Type::I32 | Type::U8 | Type::U16 | Type::U32 | Type::U64
            )
        };
        // True when the sub-expression is a plain integer literal (I64 from the
        // literal arm above). Such a literal coerces to the adjacent operand's
        // width at runtime, so we allow it without a type-mismatch constraint.
        let is_int_lit = |e: &Expr| matches!(e, Expr::Literal(AstLiteral::Int(_)));
        // Choose the "dominant" type for a sized-int binop: the concrete
        // sized width from whichever side provides one (or lt if both do —
        // they must agree; the constrain below handles mismatches).
        let dominant_sized = |l: &Type, r: &Type| -> Option<Type> {
            if is_sized_int(l) {
                Some(l.clone())
            } else if is_sized_int(r) {
                Some(r.clone())
            } else {
                None
            }
        };
        match op {
            BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                // R19 Slice B: if either operand is a sized-int type, allow
                // the other to be a plain I64 integer literal (it coerces
                // at runtime). For two sized-int operands that agree, the
                // constrain below produces no error. Different widths (e.g.
                // U8 op U32) still emit E0102 — require `as`.
                let sized = dominant_sized(&lt, &rt);
                let skip_constrain = sized.is_some()
                    && ((is_sized_int(&lt) && lt == rt)
                        || (is_sized_int(&lt) && is_int_lit(right))
                        || (is_sized_int(&rt) && is_int_lit(left)));
                if !skip_constrain {
                    self.constrain(lt.clone(), rt.clone(), "arithmetic operands");
                }
                // Fix #4: also constrain both operands to be numeric.
                // We emit a constraint to I64 which will unify cleanly
                // with I64/F64/other numeric vars, but produce a
                // mismatch if a concrete non-numeric type is present.
                // The checker (Fix #4b) performs the definitive check.
                if is_unc(&lt) || is_unc(&rt) {
                    // Pick the inner type from whichever side is Uncertain.
                    let inner = unc_inner(&lt)
                        .or_else(|| unc_inner(&rt))
                        .unwrap_or(Type::I64);
                    Type::Uncertain(Box::new(inner))
                } else if let Some(ty) = sized {
                    ty // result type is the sized-int width
                } else {
                    lt
                }
            }
            BinOp::Eq | BinOp::NotEq => {
                // R19: sized-int == sized-int or sized-int == literal: ok.
                let sized = dominant_sized(&lt, &rt);
                let skip_constrain = sized.is_some()
                    && ((is_sized_int(&lt) && lt == rt)
                        || (is_sized_int(&lt) && is_int_lit(right))
                        || (is_sized_int(&rt) && is_int_lit(left)));
                if !skip_constrain {
                    self.constrain(lt.clone(), rt.clone(), "equality operands");
                }
                if is_unc(&lt) || is_unc(&rt) {
                    Type::Uncertain(Box::new(Type::Bool))
                } else {
                    Type::Bool
                }
            }
            BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                // R19: sized-int comparisons allowed.
                let sized = dominant_sized(&lt, &rt);
                let skip_constrain = sized.is_some()
                    && ((is_sized_int(&lt) && lt == rt)
                        || (is_sized_int(&lt) && is_int_lit(right))
                        || (is_sized_int(&rt) && is_int_lit(left)));
                if !skip_constrain {
                    self.constrain(lt.clone(), rt.clone(), "comparison operands");
                }
                if is_unc(&lt) || is_unc(&rt) {
                    Type::Uncertain(Box::new(Type::Bool))
                } else {
                    Type::Bool
                }
            }
            BinOp::And | BinOp::Or => {
                // For Uncertain<bool> operands, do not constrain to plain
                // bool — soft unification handles the inner unification.
                if !is_unc(&lt) {
                    self.constrain(lt.clone(), Type::Bool, "logical operand");
                }
                if !is_unc(&rt) {
                    self.constrain(rt.clone(), Type::Bool, "logical operand");
                }
                if is_unc(&lt) || is_unc(&rt) {
                    Type::Uncertain(Box::new(Type::Bool))
                } else {
                    Type::Bool
                }
            }
            BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr => {
                // R19: allow sized-int bitwise ops; otherwise constrain to I64.
                let sized = dominant_sized(&lt, &rt);
                if let Some(ty) = sized {
                    // Both sides must be same-width sized-int or one side a literal.
                    // No cross-width constraint needed; runtime handles the masking.
                    ty
                } else {
                    self.constrain(lt.clone(), Type::I64, "bitwise operand");
                    self.constrain(rt, Type::I64, "bitwise operand");
                    Type::I64
                }
            }
        }
    }

    /// Infer the type of `expr` in `scope`, with `ret_ty` being the enclosing
    /// function's declared return type (used by `?` and `return`).
    pub fn infer_expr(&mut self, expr: &Expr, scope: &mut Scope, ret_ty: &Type) -> Type {
        match expr {
            // ── Literals ─────────────────────────────────────────────────────
            Expr::Literal(lit) => match lit {
                AstLiteral::Int(_) => Type::I64,
                AstLiteral::Float(_) => Type::F64,
                AstLiteral::Str(_) => Type::Str,
                AstLiteral::Bool(_) => Type::Bool,
                AstLiteral::Decimal(_) => Type::Decimal,
            },

            // ── Identifiers ──────────────────────────────────────────────────
            Expr::Ident(name) => {
                if let Some(ty) = scope.lookup(name) {
                    return ty.clone();
                }
                if let Some(ty) = self.module_bindings.get(name) {
                    return ty.clone();
                }
                if let Some(sig) = self.sig_instance(name) {
                    return Type::Fn(sig.params, Box::new(sig.ret));
                }
                // An unresolved name is ALREADY reported by the resolver as
                // E0001 "cannot find name" (with its own Levenshtein "did you
                // mean" suggestion, now rendered — see main.rs/lsp.rs). The
                // resolver runs first and authoritatively owns this error; an
                // empirical sweep across 12 undefined-name positions confirmed
                // infer's "cannot find value" E0101 fires iff the resolver
                // emitted E0001 at the same span — i.e. it is *strictly*
                // redundant. Emitting it here double-reports every undefined
                // name. Return a fresh var so inference continues (downstream
                // constraints still flow), but do NOT re-report the error.
                self.fresh()
            }

            // ── Bindings ─────────────────────────────────────────────────────
            Expr::Let { name, ty, value }
            | Expr::Own { name, ty, value }
            | Expr::RefBind { name, ty, value } => {
                let val_ty = self.infer_expr(value, scope, ret_ty);
                let val_ty = self.refuse_deep_binding(name, val_ty);
                // An explicit `: T` annotation constrains the value's type and
                // becomes the binding's declared type.
                if let Some(annot) = ty {
                    // CS2-3b: a bare `Dict` annotation gets one var, inferred
                    // from the binding's writes and reads.
                    self.check_dict_keys(annot, self.current_stmt_span);
                    let annot_ty = self.resolve_ast_type(annot);
                    let annot_ty = fill_bare_dicts(annot_ty, None, &mut || self.fresh());
                    // R19 Slice A: an integer LITERAL binds to any fixed-width /
                    // unsigned integer annotation (the literal takes that type),
                    // range-checked at compile time (E1900). A non-literal int →
                    // narrower/unsigned still needs an `as` cast (normal constrain
                    // → E0102). Sound by construction: arithmetic on the bound
                    // value still rejects via width-mismatch until R19 Slice B
                    // adds width-correct ops — no i64-backed half-measure (I-9).
                    if self.try_int_literal_coercion(value, &val_ty, &annot_ty) {
                        scope.bind(name.clone(), annot_ty);
                        return Type::Unit;
                    }
                    self.constrain(val_ty.clone(), annot_ty.clone(), "let type annotation");
                    scope.bind(name.clone(), annot_ty);
                } else {
                    scope.bind(name.clone(), val_ty);
                }
                Type::Unit
            }

            // ── Block ─────────────────────────────────────────────────────────
            // Phase 6: a `with <handler> { body }` has the type of its body
            // (handlers don't change the result type in this surface slice). Any
            // inline-handler arm bodies are inferred for their side effects on
            // the type environment, then discarded.
            Expr::WithHandler { handler, body } => {
                if let crate::ast::HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                    for arm in arms.iter().chain(return_arm.as_deref()) {
                        scope.push();
                        let _ = self.infer_expr(&arm.body, scope, ret_ty);
                        scope.pop();
                    }
                }
                self.infer_expr(body, scope, ret_ty)
            }

            Expr::Block(stmts) => {
                scope.push();
                let mut last = Type::Unit;
                let n = stmts.len();
                for (i, stmt) in stmts.iter().enumerate() {
                    self.current_stmt_span = stmt.span;
                    let ty = self.infer_expr(&stmt.expr, scope, ret_ty);
                    if i + 1 == n {
                        last = ty;
                    }
                }
                scope.pop();
                last
            }

            // ── Binary operations ─────────────────────────────────────────────
            Expr::BinOp { .. } => self.infer_binop_chain(expr, scope, ret_ty),

            // ── Unary operations ──────────────────────────────────────────────
            Expr::UnaryOp { op, operand } => {
                let ty = self.infer_expr(operand, scope, ret_ty);
                match op {
                    UnaryOp::Neg => {
                        // Fix #14: the checker validates numeric type; inference
                        // just passes the type through (I64 is the default integer
                        // numeric type; the checker will flag non-numeric concrete types).
                        ty
                    }
                    UnaryOp::Not => {
                        self.constrain(ty, Type::Bool, "unary not");
                        Type::Bool
                    }
                    UnaryOp::Ref | UnaryOp::RefMut => ty, // Phase 1: transparent
                    UnaryOp::BitNot => {
                        self.constrain(ty, Type::I64, "bitwise not operand");
                        Type::I64
                    }
                }
            }

            // ── Function call ─────────────────────────────────────────────────
            Expr::Call { callee, args, .. } => {
                let fn_name = match callee.as_ref() {
                    Expr::Ident(n) => Some(n.clone()),
                    // Chan::new(n) / chan<T>() both parse as StructLit { name: ... }
                    Expr::StructLit { name, fields } if fields.is_empty() => Some(name.clone()),
                    _ => None,
                };

                // Phase 13 Slice 2: P(dist op k) probability predicate.
                // The argument is a comparison whose LHS is a distribution struct —
                // skip all type-checking for these; they are handled at runtime.
                // NOTE: we intentionally do NOT infer args here because `dist <= k`
                // would constrain the distribution type against f64, causing E0102.
                if let Some(ref name) = fn_name {
                    if crate::builtins::is_prob_pred_ident(name) {
                        return Type::F64;
                    }
                }

                // Handle `chan<T>()` lowered form: "chan::<T>" → Type::Chan(T)
                if let Some(ref name) = fn_name {
                    if let Some(inner) = name
                        .strip_prefix("chan::<")
                        .and_then(|s| s.strip_suffix(">"))
                    {
                        for arg in args {
                            self.infer_expr(arg, scope, ret_ty);
                        }
                        return Type::Chan(Box::new(parse_type_str(inner)));
                    }
                }

                // R13 native FFI: `M::fn(...)` (a StructLit-callee call whose
                // `::`-joined name resolves in the native registry). Constrain
                // each arg to its FFI param type and return the FFI return type.
                // Cross-module handle mismatch (E1802) and non-representable args
                // (E1801) are diagnosed in the checker; here we just thread types.
                if let Some(ref name) = fn_name {
                    if let Some((_m, nf)) = crate::native::resolve_call(name) {
                        for (i, arg) in args.iter().enumerate() {
                            let arg_ty = self.infer_expr(arg, scope, ret_ty);
                            if let Some((pty, _mode)) = nf.params.get(i) {
                                self.constrain(arg_ty, pty.to_type(), "native FFI argument");
                            }
                        }
                        return nf.ret.to_type();
                    }
                }

                // Handle `ai_extract::<T>(prompt)` lowered form (Layer-3 ASI):
                //   "ai_extract::<T>" → Result<T, str>, taking a single str arg.
                // Mirrors the chan decode just above.  v1 T set is enforced
                // downstream in codegen (only i64/f64/bool/Uncertain<i64>/Uncertain<f64>
                // get a dispatch); the type system flows the declared T through
                // unchanged so callers see the right `Result<T, str>` shape.
                if let Some(ref name) = fn_name {
                    if let Some(inner) = name
                        .strip_prefix("ai_extract::<")
                        .and_then(|s| s.strip_suffix(">"))
                    {
                        let t = parse_type_str(inner);
                        // Constrain the (single) arg to be `str`.
                        for arg in args {
                            let arg_ty = self.infer_expr(arg, scope, ret_ty);
                            self.constrain(arg_ty, Type::Str, "arg 0 of `ai_extract`");
                        }
                        return Type::Result(Box::new(t), Box::new(Type::Str));
                    }
                }

                // `len` accepts a `str` OR any slice/array `[T]` — the interpreter
                // handles both, but the builtin signature only lists `str`. Special
                // -case the arg type so `len(my_array)` type-checks. A non-slice arg
                // flows through the declared `str` param, so `len(42)` still errors.
                if let Some(ref name) = fn_name {
                    if name == "len" && args.len() == 1 {
                        let arg_ty = self.infer_expr(&args[0], scope, ret_ty);
                        match arg_ty {
                            Type::Slice(_) => {}
                            // Unresolved here (`?`/match binding): decide after
                            // solving, when it may have become `[T]`.
                            Type::Var(_) => {
                                let span = self.current_stmt_span;
                                self.len_obligations.push((arg_ty, span));
                            }
                            _ => self.constrain(arg_ty, Type::Str, "arg 0 of `len`"),
                        }
                        return Type::I64;
                    }
                }

                // `to_str` is polymorphic over scalars (i64/i32/.../f64/bool) —
                // the interpreter dispatches on the runtime value (BUG_HUNT #29),
                // so picking `to_str_f64`/`to_str_bool` is never required. Accept
                // any scalar without constraint; a non-scalar arg still flows to
                // the declared `i64` param below and errors, so `to_str([1,2])`
                // is rejected.
                if let Some(ref name) = fn_name {
                    if name == "to_str" && args.len() == 1 {
                        let arg_ty = self.infer_expr(&args[0], scope, ret_ty);
                        // `is_str` as well as `is_scalar`: `to_str` of a `str` is
                        // the identity. See interp/builtins.rs for why refusing it
                        // was a wart (9 of 36 measured first errors).
                        if arg_ty.is_scalar() || arg_ty.is_str() {
                            return Type::Str;
                        }
                        // Not yet RESOLVED is not the same as wrong. A value bound
                        // by a pattern (`Err(e) => to_str(e)`) is still a type var
                        // here and only becomes `str` when the constraints solve —
                        // so constraining it to `i64` below reported
                        // "expected i64, found str" for exactly the identity call
                        // the arm above exists to allow. Measured: this was the
                        // remaining half of the `to_str`-of-a-`str` wart, still
                        // firing on the benchmark after the widening landed. A
                        // genuinely non-scalar arg (`to_str([1, 2])`) resolves to a
                        // concrete type and is still rejected by the checker's own
                        // argument loop, which skips only scalars and `str`.
                        if type_is_unresolved(&arg_ty) {
                            return Type::Str;
                        }
                        // Non-scalar: constrain the ALREADY-inferred arg to the
                        // declared `i64` param right here — reproducing the exact
                        // E0102 the general arg loop below would emit — instead of
                        // falling through to re-infer the arg. Falling through
                        // double-visited the arg: a `to_str(<undefined>)` then
                        // emitted its E0101 "cannot find value" diagnostic TWICE
                        // (once here, once below). `to_str` is always str-returning.
                        self.constrain(arg_ty, Type::I64, "arg 0 of `to_str`");
                        return Type::Str;
                    }
                }
                // CS2-6: a callee name bound by a local or a param that holds a
                // fn value is typed by that binding, never by the top-level fn
                // or builtin of the same name (a call by name runs the local).
                let local_callee = matches!(callee.as_ref(), Expr::Ident(n)
                    if scope.lookup(n).is_some_and(|t| matches!(t, Type::Fn(..) | Type::Var(_))));
                let callee_ty = self.infer_expr(callee, scope, ret_ty);

                if let Some(name) = fn_name.filter(|_| !local_callee) {
                    // CS-4: the sig of the fn this call reaches — the builtin's
                    // for a plain call to a builtin name, the user fn's otherwise.
                    let prefers_builtin = crate::builtins::call_prefers_builtin(
                        &name,
                        crate::builtins::call_has_mut_arg(args),
                    );
                    let user_sig = if prefers_builtin {
                        None
                    } else {
                        self.user_fn_sigs.get(&name)
                    };
                    // For generic functions: instantiate with fresh type vars per
                    // call site, using the type-param names of the sig reached.
                    // `generic_fn_params` keys are top-level user fns, all of
                    // which are in `user_fn_sigs`; any other `fn_sigs` entry
                    // takes the builtin's names (none for a non-builtin).
                    let (sig, param_names, reaches_builtin) = match user_sig {
                        Some(s) => (
                            Some(s.clone()),
                            self.generic_fn_params
                                .get(&name)
                                .cloned()
                                .unwrap_or_default(),
                            false,
                        ),
                        None => (
                            self.fn_sigs.get(&name).cloned(),
                            self.builtin_generic_params
                                .get(&name)
                                .cloned()
                                .unwrap_or_default(),
                            self.builtin_generic_params.contains_key(&name),
                        ),
                    };
                    // CS2-3b: a generalized bare-`Dict` fn's implicit params
                    // are instantiated per call like its declared ones.
                    let declared_n = param_names.len();
                    let mut param_names = param_names;
                    if !prefers_builtin {
                        if let Some(extra) = self.implicit_fn_params.get(&name) {
                            param_names.extend(extra.iter().cloned());
                        }
                    }
                    if let Some(sig) = sig {
                        let (inst_sig, var_map) = if !param_names.is_empty() {
                            self.instantiate_sig(&sig, &param_names)
                        } else {
                            (sig, HashMap::new())
                        };
                        // CS2-3a: a numeric key fn's return must resolve to a number.
                        if let Some(&v) = var_map.get(NUM_PARAM) {
                            self.num_obligations
                                .push((Type::Var(v), self.current_stmt_span));
                        }
                        // ── Layer-1 ASI: confidence propagation note ─────────
                        // When any argument has type `Uncertain<_>`, the call's
                        // result confidence should logically be the product of the
                        // input confidences (cf. PRD AI_Language_Plan.md §1360-).
                        // Phase-1 leaves this informational; we do not enforce a
                        // type-system rule yet (no monomorphisation over confidence).
                        // A future pass can walk `args` for `Type::Uncertain(_)`
                        // and emit runtime instrumentation to multiply confidences
                        // at call boundaries.
                        // Fix #16: include function name and param index in
                        // the constraint origin for clearer error messages.
                        for (i, (arg, param_ty)) in
                            args.iter().zip(inst_sig.params.iter()).enumerate()
                        {
                            let arg_ty = self.infer_expr(arg, scope, ret_ty);
                            // R19: a literal arg coerces to a fixed-width/unsigned param.
                            if self.try_int_literal_coercion(arg, &arg_ty, param_ty) {
                                continue;
                            }
                            // CS2-3a: a closure of the wrong arity is the checker's
                            // E0306, which names the shape to write; unifying it here
                            // would add a bare E0102 for the same call.
                            if matches!((&arg_ty, param_ty),
                                (Type::Fn(a, _), Type::Fn(p, _)) if a.len() != p.len())
                            {
                                continue;
                            }
                            // CS2-3a: a builtin discards the result of a closure
                            // param declared with a unit return, so that return
                            // is a fresh var the closure's body may fix to anything.
                            let freed;
                            let param_ty = match param_ty {
                                Type::Fn(ps, r) if reaches_builtin && **r == Type::Unit => {
                                    freed = Type::Fn(ps.clone(), Box::new(self.fresh()));
                                    &freed
                                }
                                p => p,
                            };
                            self.constrain(
                                arg_ty,
                                param_ty.clone(),
                                &format!("arg {i} of `{name}`"),
                            );
                        }
                        let ret = inst_sig.ret.clone();
                        // Record instantiation for the mono pass (resolved later).
                        //
                        // BUILTINS are excluded. Their type-variable slots are
                        // instantiated above so inference binds `T` once and
                        // checks it against the rest -- but there is no AST body
                        // to specialize, so queueing one makes the mono pass emit
                        // a stub that shadows the real extern. That is how a
                        // native `dict_set` silently stored nothing while the
                        // interpreter stored 20.
                        if !var_map.is_empty() && !reaches_builtin {
                            // CS2-3b: only the declared params; an implicit one
                            // (a bare `Dict`'s value) has no AST name to specialize.
                            let var_ids: Vec<u32> = param_names[..declared_n]
                                .iter()
                                .filter_map(|n| var_map.get(n).copied())
                                .collect();
                            // Store pending instantiation — vars will be resolved after solving.
                            if !var_ids.is_empty() {
                                self.call_instantiations
                                    .push((name, var_ids.iter().map(|&v| Type::Var(v)).collect()));
                            }
                        }
                        return ret;
                    }
                }
                // A callee with no signature by name: a local or param, an
                // index, a call, a field, a lambda. CS2-6: once the
                // constraints are solved, a callee that resolves to a fn type
                // checks the call's arity and argument types, and gives its
                // return type ([`InferCtx::solve`]).
                let call_args: Vec<Type> = args
                    .iter()
                    .map(|arg| self.infer_expr(arg, scope, ret_ty))
                    .collect();
                let ret = self.fresh();
                self.call_obligations.push(CallObligation {
                    callee: callee_ty,
                    args: call_args,
                    ret: ret.clone(),
                    span: self.current_stmt_span,
                });
                ret
            }

            // ── Method call (Phase 1: treat as top-level fn lookup) ───────────
            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                let recv_ty = self.infer_expr(receiver, scope, ret_ty);
                if matches!(recv_ty, Type::Var(_)) {
                    self.method_receiver_vars
                        .push((receiver.as_ref() as *const Expr as usize, recv_ty.clone()));
                }
                // Special-case Chan<T> methods: recv() → T, send(T) → Unit, clone() → Chan<T>.
                match (method.as_str(), &recv_ty) {
                    ("recv", Type::Chan(inner)) => {
                        return *inner.clone();
                    }
                    ("send", Type::Chan(inner)) => {
                        for arg in args {
                            let arg_ty = self.infer_expr(arg, scope, ret_ty);
                            self.constrain(arg_ty, *inner.clone(), "chan.send argument");
                        }
                        return Type::Unit;
                    }
                    ("clone", Type::Chan(inner)) => {
                        return Type::Chan(inner.clone());
                    }
                    _ => {}
                }
                // DynTrait receiver: look up trait method signature.
                if let Type::DynTrait(trait_name) = &recv_ty {
                    if let Some(method_sigs) = self.trait_method_sigs.get(trait_name).cloned() {
                        if let Some(sig) = method_sigs.get(method.as_str()).cloned() {
                            for (arg, param_ty) in args.iter().zip(sig.params.iter()) {
                                let arg_ty = self.infer_expr(arg, scope, ret_ty);
                                self.constrain(arg_ty, param_ty.clone(), "trait method argument");
                            }
                            return sig.ret.clone();
                        }
                    }
                    // Unknown trait method — infer args and return fresh.
                    for arg in args {
                        self.infer_expr(arg, scope, ret_ty);
                    }
                    return self.fresh();
                }
                if let Some(sig) = self.sig_instance(method) {
                    for (arg, param_ty) in args.iter().zip(sig.params.iter()) {
                        let arg_ty = self.infer_expr(arg, scope, ret_ty);
                        self.constrain(arg_ty, param_ty.clone(), "method argument");
                    }
                    sig.ret.clone()
                } else {
                    for arg in args {
                        self.infer_expr(arg, scope, ret_ty);
                    }
                    self.fresh()
                }
            }

            // ── ? operator ────────────────────────────────────────────────────
            Expr::Question(inner) => {
                let inner_ty = self.infer_expr(inner, scope, ret_ty);
                let ok_var = self.fresh();
                let err_var = self.fresh();
                // inner must be Result<ok_var, err_var>
                self.constrain(
                    inner_ty,
                    Type::Result(Box::new(ok_var.clone()), Box::new(err_var.clone())),
                    "? inner type",
                );
                // enclosing fn must return Result<_, err_var>
                let ret_ok_var = self.fresh();
                self.constrain(
                    ret_ty.clone(),
                    Type::Result(Box::new(ret_ok_var), Box::new(err_var)),
                    "? return context",
                );
                ok_var // ? unwraps to Ok value
            }

            // ── Match ─────────────────────────────────────────────────────────
            Expr::Match { subject, arms } => {
                // Fix #8: pass subject type into arm inference so patterns can
                // constrain it and idents can be bound to the right type.
                let subj_ty = self.infer_expr(subject, scope, ret_ty);
                if arms.is_empty() {
                    return Type::Unit;
                }
                let first_ty =
                    self.infer_match_arm_with_subj(&arms[0], scope, ret_ty, Some(&subj_ty));
                for arm in &arms[1..] {
                    let arm_ty = self.infer_match_arm_with_subj(arm, scope, ret_ty, Some(&subj_ty));
                    self.constrain(arm_ty, first_ty.clone(), "match arm type");
                }
                first_ty
            }

            // ── If / if-else ──────────────────────────────────────────────────
            Expr::If { cond, then, else_ } => {
                let cond_ty = self.infer_expr(cond, scope, ret_ty);
                self.constrain(cond_ty, Type::Bool, "if condition");
                let then_ty = self.infer_expr(then, scope, ret_ty);
                if let Some(else_expr) = else_ {
                    let else_ty = self.infer_expr(else_expr, scope, ret_ty);
                    self.constrain(else_ty, then_ty.clone(), "if-else branch types");
                    then_ty
                } else {
                    // Fix #9: if without else always yields Unit, since there
                    // is no value to produce when the condition is false.
                    self.constrain(then_ty, Type::Unit, "if-without-else body");
                    Type::Unit
                }
            }

            // ── Return ────────────────────────────────────────────────────────
            Expr::Return(val) => {
                let val_ty = val
                    .as_ref()
                    .map(|v| self.infer_expr(v, scope, ret_ty))
                    .unwrap_or(Type::Unit);
                // R19 NOTE: unsigned/fixed-width RETURNS are deferred — the return
                // path has a separate checker E0307 + fn-body-type check beyond this
                // infer constraint, so a literal-coercion here alone is insufficient
                // (and unsound to half-do). Tracked as Slice A-cont in R19.
                self.constrain(val_ty, ret_ty.clone(), "return value");
                // Diverging: `Never` unifies with any sibling arm/branch type
                // (`None => return Err(..)` next to `Some(x) => x`).
                Type::Never
            }

            // ── Ok / Err / Some / None ────────────────────────────────────────
            Expr::Ok(inner) => {
                let ok_ty = self.infer_expr(inner, scope, ret_ty);
                let err_var = self.fresh();
                Type::Result(Box::new(ok_ty), Box::new(err_var))
            }
            Expr::Err(inner) => {
                let err_ty = self.infer_expr(inner, scope, ret_ty);
                let ok_var = self.fresh();
                Type::Result(Box::new(ok_var), Box::new(err_ty))
            }
            Expr::Some(inner) => {
                let inner_ty = self.infer_expr(inner, scope, ret_ty);
                Type::Option(Box::new(inner_ty))
            }
            Expr::None => {
                let var = self.fresh();
                Type::Option(Box::new(var))
            }

            // ── Array literal ─────────────────────────────────────────────────
            Expr::Array(elems) => {
                if elems.is_empty() {
                    let var = self.fresh();
                    return Type::Slice(Box::new(var));
                }
                let first_ty = self.infer_expr(&elems[0], scope, ret_ty);
                for elem in &elems[1..] {
                    let elem_ty = self.infer_expr(elem, scope, ret_ty);
                    self.constrain(elem_ty, first_ty.clone(), "array element type");
                }
                Type::Slice(Box::new(first_ty))
            }

            Expr::Tuple(elems) => {
                // Heterogeneous: each element keeps its own inferred type.
                Type::Tuple(
                    elems
                        .iter()
                        .map(|e| self.infer_expr(e, scope, ret_ty))
                        .collect(),
                )
            }

            // ── Struct literal ─────────────────────────────────────────────────
            Expr::StructLit { name, fields } => {
                // Fix #1: look up the declared fields and constrain each
                // field expression against the declared type.

                // "EnumName::VariantName" → Type::Enum("EnumName")
                if name.contains("::") {
                    let parts: Vec<&str> = name.splitn(2, "::").collect();
                    let enum_name = parts[0].to_string();
                    let variant_name = parts.get(1).copied().unwrap_or("").to_string();
                    if self.enum_variants.contains_key(&enum_name) {
                        // Fix #2: use known payload field types when available.
                        // `enum_variant_fields[enum][variant]` exists iff the
                        // variant exists (an empty Vec = a fieldless variant), so
                        // distinguish "unknown variant" from "no payload".
                        let known_variant = self
                            .enum_variant_fields
                            .get(&enum_name)
                            .map(|m| m.get(&variant_name));
                        let declared_payload: Vec<(String, Type)> = match known_variant {
                            Some(Some(p)) => p.clone(),
                            _ => Vec::new(),
                        };
                        // Whether we have an authoritative field list for this
                        // variant (so an unknown field name can be flagged like
                        // struct E0101 does — E0404 here for an enum variant).
                        let variant_known = matches!(known_variant, Some(Some(_)));

                        for (fname, fexpr) in fields {
                            let inferred_ty = self.infer_expr(fexpr, scope, ret_ty);
                            if let Some((_, decl_ty)) =
                                declared_payload.iter().find(|(dn, _)| dn == fname)
                            {
                                self.constrain(inferred_ty, decl_ty.clone(), "enum variant field");
                            } else if variant_known {
                                // A field name not on this variant: `S::A { y }`
                                // when `A`'s only field is `x`. Was silently
                                // accepted (then built a bogus value); flag E0404.
                                let known: Vec<String> =
                                    declared_payload.iter().map(|(n, _)| n.clone()).collect();
                                let known_list = if known.is_empty() {
                                    "none (this variant has no fields)".to_string()
                                } else {
                                    known.join(", ")
                                };
                                let span = self.current_stmt_span;
                                self.errors.push(
                                    InferError::new(
                                        E0404,
                                        format!(
                                            "variant `{enum_name}::{variant_name}` has no field `{fname}` (known fields: {known_list})"
                                        ),
                                    )
                                    .with_span(span),
                                );
                            }
                        }
                        return Type::Enum(enum_name);
                    }
                }

                // Struct literal: constrain each provided field against declared type.
                // CS2-5: a generic struct (e.g. `Pair<A, B>`) gets a fresh Var
                // per type param, substituted into the field types (stored as
                // `Deferred("A")`, also inside `[A]` etc.); the literal's type is
                // `App(name, vars)`, so an annotation `Pair<i64, str>` is compared.
                let raw_declared: Vec<(String, Type)> =
                    self.struct_fields.get(name).cloned().unwrap_or_default();
                let mut app_vars: Option<Vec<Type>> = None;
                let declared: Vec<(String, Type)> = if let Some(param_names) =
                    self.struct_generic_params.get(name).cloned()
                {
                    let vars: Vec<Type> = param_names.iter().map(|_| self.fresh()).collect();
                    let declared = raw_declared
                        .into_iter()
                        .map(|(fname, fty)| (fname, subst_struct_params(fty, &param_names, &vars)))
                        .collect();
                    app_vars = Some(vars);
                    declared
                } else {
                    raw_declared
                };

                // Track which declared fields were provided for missing-field detection.
                let mut provided: std::collections::HashSet<String> =
                    std::collections::HashSet::new();

                for (fname, fexpr) in fields {
                    let inferred_ty = self.infer_expr(fexpr, scope, ret_ty);
                    provided.insert(fname.clone());

                    if let Some((_, decl_ty)) = declared.iter().find(|(dn, _)| dn == fname) {
                        // R19: a literal field value coerces to a fixed-width/unsigned field type.
                        let decl_ty = decl_ty.clone();
                        if !self.try_int_literal_coercion(fexpr, &inferred_ty, &decl_ty) {
                            self.constrain(inferred_ty, decl_ty, "struct field");
                        }
                    } else if !declared.is_empty() {
                        // Unknown field name (only report if we know the struct).
                        let known: Vec<String> = declared.iter().map(|(n, _)| n.clone()).collect();
                        let span = self.current_stmt_span;
                        self.errors.push(
                            InferError::new(
                                E0101,
                                format!(
                                    "struct `{name}` has no field `{fname}` (known fields: {})",
                                    known.join(", ")
                                ),
                            )
                            .with_span(span),
                        );
                    }
                }

                // Report missing required fields (only when struct is known).
                if !declared.is_empty() {
                    for (dname, _) in &declared {
                        if !provided.contains(dname) {
                            let span = self.current_stmt_span;
                            self.errors.push(
                                InferError::new(
                                    E0101,
                                    format!(
                                        "struct `{name}` literal is missing required field `{dname}`",
                                    ),
                                )
                                .with_span(span),
                            );
                        }
                    }
                }

                match app_vars {
                    Some(vars) => Type::App(name.clone(), vars),
                    None => Type::Struct(name.clone()),
                }
            }

            // ── Field access ──────────────────────────────────────────────────
            Expr::FieldAccess { receiver, field } => {
                let recv_ty = self.infer_expr(receiver, scope, ret_ty);
                match &recv_ty {
                    Type::Struct(name) => {
                        if let Some(fields) = self.struct_fields.get(name) {
                            if let Some((_, field_ty)) = fields.iter().find(|(n, _)| n == field) {
                                // For generic struct fields stored as TypeParam, return a fresh Var
                                // so the type can be solved by unification at the use site.
                                if matches!(field_ty, Type::TypeParam(_)) {
                                    self.fresh()
                                } else if let Some(ps) = self.struct_generic_params.get(name) {
                                    // CS2-3b: a bare struct name leaves its implicit
                                    // params (bare-`Dict` fields) open: fresh per read,
                                    // as the untyped dict was.
                                    let (field_ty, ps) = (field_ty.clone(), ps.clone());
                                    let args: Vec<Type> = ps
                                        .iter()
                                        .map(|p| {
                                            if p.starts_with("__dict") {
                                                self.fresh()
                                            } else {
                                                Type::Deferred(p.clone())
                                            }
                                        })
                                        .collect();
                                    subst_struct_params(field_ty, &ps, &args)
                                } else {
                                    field_ty.clone()
                                }
                            } else {
                                // Unknown field on a known struct in field-access
                                // position is ALREADY reported by the checker as
                                // the canonical E0401 "struct '…' has no field
                                // '…'", with the known-fields list in its
                                // structured `fix`/help. Don't also emit an E0101
                                // here (that double-reported the same access, and
                                // overloaded E0101 — a name-resolution code — for a
                                // field error). Return a fresh var so inference
                                // continues; the checker owns the diagnostic. (The
                                // struct-LITERAL field cases below stay on infer —
                                // the checker does not cover unknown/missing fields
                                // in a literal.)
                                self.fresh()
                            }
                        } else {
                            self.fresh()
                        }
                    }
                    // CS2-5: a field of an applied generic struct has the
                    // struct's type args substituted for its params.
                    Type::App(name, args) => {
                        let field_ty = self
                            .struct_fields
                            .get(name)
                            .and_then(|fs| fs.iter().find(|(n, _)| n == field))
                            .map(|(_, t)| t.clone());
                        match (field_ty, self.struct_generic_params.get(name)) {
                            (Some(t), Some(ps)) => subst_struct_params(t, ps, args),
                            _ => self.fresh(),
                        }
                    }
                    // Tuple field access: `t.0` / `t.1`. The field name is
                    // a numeric index (the parser stores it as a string).
                    // Returning the indexed element's actual type avoids the
                    // "fresh-var leak" bug where `let (a, b) = tup; len(a)`
                    // would constrain `a` to `str` via len's fallback, then
                    // unification with a real Slice later fails.
                    Type::Tuple(elems) => {
                        if let Ok(i) = field.parse::<usize>() {
                            if let Some(t) = elems.get(i) {
                                return t.clone();
                            }
                        }
                        self.fresh()
                    }
                    // Phase 1: unknown receiver type — defer.
                    _ => self.fresh(),
                }
            }

            // ── Index ─────────────────────────────────────────────────────────
            Expr::Index { receiver, index } => {
                // Phase 13 Slice 2: E[dist] and Var[dist] distribution moment
                // predicates. Skip the normal array/I64 constraints; infer the
                // inner expression for its side-effects on scope but return F64.
                if let Expr::Ident(tag) = receiver.as_ref() {
                    if crate::builtins::is_prob_pred_ident(tag) {
                        self.infer_expr(index, scope, ret_ty);
                        return Type::F64;
                    }
                }
                let recv_ty = self.infer_expr(receiver, scope, ret_ty);
                let idx_ty = self.infer_expr(index, scope, ret_ty);
                self.constrain(idx_ty, Type::I64, "slice index");
                match recv_ty {
                    Type::Slice(elem) => *elem,
                    _ => self.fresh(),
                }
            }

            // ── Spawn (Phase 1: sequential stub) ──────────────────────────────
            Expr::Spawn(body) => {
                self.infer_expr(body, scope, ret_ty);
                Type::Unit
            }

            // ── Select ────────────────────────────────────────────────────────
            Expr::Select(arms) => {
                for arm in arms {
                    self.infer_expr(&arm.recv, scope, ret_ty);
                    self.infer_expr(&arm.body, scope, ret_ty);
                }
                Type::Unit
            }

            // ── Comptime ──────────────────────────────────────────────────────
            Expr::Comptime(body) => self.infer_expr(body, scope, ret_ty),

            // ── Inline asm (R17 Slice 1) — always Unit ───────────────────────
            Expr::InlineAsm { .. } => Type::Unit,

            // ── Lambda ────────────────────────────────────────────────────────
            Expr::Lambda {
                params,
                body,
                captures: _,
            } => {
                let param_types: Vec<Type> = params.iter().map(|_| self.fresh()).collect();
                // Fix #6: create a fresh return-type variable for the lambda
                // instead of reusing the enclosing function's ret_ty. This
                // prevents `?` and `return` inside the lambda from bleeding
                // their constraints into the outer function's return type.
                let lambda_ret_var = self.fresh();
                scope.push();
                for (p, ty) in params.iter().zip(param_types.iter()) {
                    scope.bind(p.name.clone(), ty.clone());
                }
                let body_ty = self.infer_expr(body, scope, &lambda_ret_var);
                scope.pop();
                Type::Fn(param_types, Box::new(body_ty))
            }

            // ── While loop ────────────────────────────────────────────────────
            Expr::While { cond, body } => {
                let cond_ty = self.infer_expr(cond, scope, ret_ty);
                self.constrain(cond_ty, Type::Bool, "while condition");
                scope.push();
                // Track the span per statement, exactly as `Expr::Block` does.
                // Without this every type error inside a loop body carried the
                // LOOP's span, so the diagnostic named the `for`/`while` line
                // instead of the failing line. Two consequences, both measured:
                // the reader is sent to a line that is already correct, and
                // `collapse_refined_type_errors` — which keys on the span —
                // could not pair the bare E0102 with the hint-bearing checker
                // diagnostic for the same failure, so both were reported with
                // the useless one first.
                let outer_span = self.current_stmt_span;
                for stmt in body {
                    self.current_stmt_span = stmt.span;
                    self.infer_expr(&stmt.expr, scope, ret_ty);
                }
                self.current_stmt_span = outer_span;
                scope.pop();
                Type::Unit
            }

            // ── While-let loop ────────────────────────────────────────────────
            // `while let Some(x) = expr { body }` — expr is Option<T>,
            // pattern binds T; loop evaluates as unit.
            Expr::WhileLet {
                pattern,
                expr,
                body,
            } => {
                let expr_ty = self.infer_expr(expr, scope, ret_ty);
                scope.push();
                self.bind_pattern_with_subj(pattern, scope, Some(&expr_ty));
                // Track the span per statement, exactly as `Expr::Block` does.
                // Without this every type error inside a loop body carried the
                // LOOP's span, so the diagnostic named the `for`/`while` line
                // instead of the failing line. Two consequences, both measured:
                // the reader is sent to a line that is already correct, and
                // `collapse_refined_type_errors` — which keys on the span —
                // could not pair the bare E0102 with the hint-bearing checker
                // diagnostic for the same failure, so both were reported with
                // the useless one first.
                let outer_span = self.current_stmt_span;
                for stmt in body {
                    self.current_stmt_span = stmt.span;
                    self.infer_expr(&stmt.expr, scope, ret_ty);
                }
                self.current_stmt_span = outer_span;
                scope.pop();
                Type::Unit
            }

            // ── For loop (range iteration) ───────────────────────────────────
            Expr::For {
                var,
                start,
                end,
                body,
                ..
            } => {
                let start_ty = self.infer_expr(start, scope, ret_ty);
                let end_ty = self.infer_expr(end, scope, ret_ty);
                self.constrain(start_ty, Type::I64, "for range start");
                self.constrain(end_ty, Type::I64, "for range end");
                scope.push();
                scope.bind(var.clone(), Type::I64);
                // Track the span per statement, exactly as `Expr::Block` does.
                // Without this every type error inside a loop body carried the
                // LOOP's span, so the diagnostic named the `for`/`while` line
                // instead of the failing line. Two consequences, both measured:
                // the reader is sent to a line that is already correct, and
                // `collapse_refined_type_errors` — which keys on the span —
                // could not pair the bare E0102 with the hint-bearing checker
                // diagnostic for the same failure, so both were reported with
                // the useless one first.
                let outer_span = self.current_stmt_span;
                for stmt in body {
                    self.current_stmt_span = stmt.span;
                    self.infer_expr(&stmt.expr, scope, ret_ty);
                }
                self.current_stmt_span = outer_span;
                scope.pop();
                Type::Unit
            }

            // ── Break / Continue ─────────────────────────────────────────────
            Expr::Break | Expr::Continue => Type::Never,

            // ── Assign (rebind without let) ───────────────────────────────────
            Expr::Assign { name, value } => {
                let val_ty = self.infer_expr(value, scope, ret_ty);
                // Fix #5: constrain new value type to match the existing type
                // rather than overwriting the scope binding. The variable keeps
                // its original declared type.
                if let Some(prev_ty) = scope.lookup(name).cloned() {
                    self.constrain(val_ty, prev_ty, "assignment");
                }
                Type::Unit
            }
            Expr::AssignTo { place, value } => {
                // `place[i] = v` / `place.f = v`: the value must match the
                // element/field type the place denotes.
                let place_ty = self.infer_expr(place, scope, ret_ty);
                let val_ty = self.infer_expr(value, scope, ret_ty);
                self.constrain(val_ty, place_ty, "place assignment");
                Type::Unit
            }

            // ── FmtStr: infer type of each interpolated part, return Str ────
            Expr::FmtStr { parts } => {
                for part in parts {
                    if let FmtPart::Expr(e) = part {
                        self.infer_expr(e, scope, ret_ty);
                    }
                }
                Type::Str
            }
        }
    }

    // ── Match arm helper ──────────────────────────────────────────────────────

    #[allow(dead_code)]
    fn infer_match_arm(&mut self, arm: &MatchArm, scope: &mut Scope, ret_ty: &Type) -> Type {
        self.infer_match_arm_with_subj(arm, scope, ret_ty, None)
    }

    fn infer_match_arm_with_subj(
        &mut self,
        arm: &MatchArm,
        scope: &mut Scope,
        ret_ty: &Type,
        subj_ty: Option<&Type>,
    ) -> Type {
        scope.push();
        self.bind_pattern_with_subj(&arm.pattern, scope, subj_ty);
        if let Some(guard) = &arm.guard {
            let guard_ty = self.infer_expr(guard, scope, ret_ty);
            self.constrain(guard_ty, Type::Bool, "match guard");
        }
        let body_ty = self.infer_expr(&arm.body, scope, ret_ty);
        scope.pop();
        body_ty
    }

    /// Bind identifiers introduced by a pattern into `scope`.
    #[allow(dead_code)]
    fn bind_pattern(&mut self, pattern: &Pattern, scope: &mut Scope) {
        self.bind_pattern_with_subj(pattern, scope, None);
    }

    /// Bind identifiers introduced by a pattern into `scope`, optionally
    /// constraining them using the known subject type (Fix #8).
    fn bind_pattern_with_subj(
        &mut self,
        pattern: &Pattern,
        scope: &mut Scope,
        subj_ty: Option<&Type>,
    ) {
        match pattern {
            Pattern::Wildcard | Pattern::Literal(_) | Pattern::None => {}
            // Fix #8: bind the ident to the subject type when known.
            Pattern::Ident(name) => {
                let var = if let Some(st) = subj_ty {
                    st.clone()
                } else {
                    self.fresh()
                };
                scope.bind(name.clone(), var);
            }
            // Fix #8: for Ok/Err/Some patterns, constrain the subject type.
            Pattern::Ok(inner) => {
                let inner_var = self.fresh();
                let err_var = self.fresh();
                if let Some(st) = subj_ty {
                    self.constrain(
                        st.clone(),
                        Type::Result(Box::new(inner_var.clone()), Box::new(err_var)),
                        "Ok pattern",
                    );
                }
                self.bind_pattern_with_subj(inner, scope, Some(&inner_var));
            }
            Pattern::Err(inner) => {
                let ok_var = self.fresh();
                let err_var = self.fresh();
                if let Some(st) = subj_ty {
                    self.constrain(
                        st.clone(),
                        Type::Result(Box::new(ok_var), Box::new(err_var.clone())),
                        "Err pattern",
                    );
                }
                self.bind_pattern_with_subj(inner, scope, Some(&err_var));
            }
            Pattern::Some(inner) => {
                let inner_var = self.fresh();
                if let Some(st) = subj_ty {
                    self.constrain(
                        st.clone(),
                        Type::Option(Box::new(inner_var.clone())),
                        "Some pattern",
                    );
                }
                self.bind_pattern_with_subj(inner, scope, Some(&inner_var));
            }
            Pattern::Struct { fields, .. } => {
                for (_, pat) in fields {
                    self.bind_pattern_with_subj(pat, scope, None);
                }
            }
            Pattern::Tuple(pats) => {
                for pat in pats {
                    self.bind_pattern_with_subj(pat, scope, None);
                }
            }
        }
    }

    // ── Program-level inference ───────────────────────────────────────────────

    /// Run inference over the whole program:
    /// 1. Collect signatures (first pass).
    /// 2. Infer each function body.
    /// 3. Solve constraints.
    ///
    /// Returns the resulting `Substitution`; errors are in `self.errors`.
    pub fn infer_program(&mut self, program: &Program) -> Substitution {
        self.collect_sigs(program);
        // Process module-level let bindings first so functions can reference them.
        let mut module_scope = Scope::new();
        for item in &program.items {
            if let Item::LetDef { name, value, .. } = item {
                let ty = self.infer_expr(value, &mut module_scope, &Type::Unit);
                self.module_bindings.insert(name.clone(), ty);
            }
        }
        // CS2-3b: the fns whose signature holds a bare `Dict` are inferred
        // first, callees before callers (a recursive group together), and
        // each group's vars solved and generalized before its callers.
        let dict_fns: HashMap<&str, &FnDef> = program
            .items
            .iter()
            .filter_map(|i| match i {
                Item::FnDef(f) if self.dict_owned_vars.contains_key(&f.name) => {
                    Some((f.name.as_str(), f))
                }
                _ => None,
            })
            .collect();
        let order: Vec<&str> = program
            .items
            .iter()
            .filter_map(|i| match i {
                Item::FnDef(f) if dict_fns.contains_key(f.name.as_str()) => Some(f.name.as_str()),
                _ => None,
            })
            .collect();
        for group in callee_first_groups(&order, &dict_fns) {
            for name in &group {
                self.infer_fn(dict_fns[name], true);
            }
            self.solve_pending();
            self.generalize_dict_vars(&group);
        }
        for item in &program.items {
            match item {
                Item::FnDef(f)
                    if !dict_fns
                        .get(f.name.as_str())
                        .is_some_and(|d| std::ptr::eq(*d, f)) =>
                {
                    self.infer_fn(f, true)
                }
                Item::ImplBlock(blk) => {
                    for m in &blk.methods {
                        self.infer_fn(m, false);
                    }
                }
                _ => {}
            }
        }
        let subst = self.solve();
        for (recv, ty) in std::mem::take(&mut self.method_receiver_vars) {
            // A fn body sees its declared `T` as `Deferred("T")`; the checker
            // keeps only names that are the enclosing fn's type params.
            if let Type::TypeParam(n) | Type::Deferred(n) = subst.apply(&ty) {
                self.method_receiver_type_params.insert(recv, n);
            }
        }
        // Resolve pending generic instantiations now that the substitution is final.
        for (_, type_args) in &mut self.call_instantiations {
            for ty in type_args.iter_mut() {
                *ty = subst.apply(ty);
            }
        }
        subst
    }

    /// A context for typing predicates (CS-1/CS-2): every signature, struct
    /// and refinement of `program` collected, and the module-level `let`s
    /// typed, as [`InferCtx::infer_program`] would before the fn bodies.
    pub fn for_predicates(file: impl Into<String>, program: &Program) -> Self {
        let mut ctx = Self::new(file);
        ctx.collect_sigs(program);
        let mut module_scope = Scope::new();
        for item in &program.items {
            if let Item::LetDef { name, value, .. } = item {
                let ty = ctx.infer_expr(value, &mut module_scope, &Type::Unit);
                ctx.module_bindings.insert(name.clone(), ty);
            }
        }
        let subst = ctx.solve();
        for ty in ctx.module_bindings.values_mut() {
            *ty = subst.apply(ty);
        }
        // Errors in the program itself are `infer_program`'s to report.
        ctx.errors.clear();
        ctx
    }

    /// Type the predicate `pred` in a scope of module items, builtins and
    /// `bindings` (each name with its declared type), tagging what it finds
    /// with `span`. Nothing here is reported: the caller words the refusal.
    pub fn infer_predicate(
        &mut self,
        pred: &Expr,
        bindings: &[(String, AxonType)],
        span: crate::span::Span,
    ) -> PredicateTyping {
        let mut scope = Scope::new();
        for (name, ty) in bindings {
            let ty = self.resolve_ast_type(ty);
            scope.bind(name.clone(), ty);
        }
        self.current_stmt_span = span;
        self.predicate_int_operands = Some(Vec::new());
        let ty = self.infer_expr(pred, &mut scope, &Type::Unit);
        let seen = self.predicate_int_operands.take().unwrap_or_default();
        let subst = self.solve();
        let int_against_float = seen
            .into_iter()
            .map(|(n, other, span)| (n, subst.apply(&other), span))
            .filter(|(_, other, _)| matches!(other, Type::F64 | Type::F32))
            .collect();
        PredicateTyping {
            ty: subst.apply(&ty),
            int_against_float,
            errors: std::mem::take(&mut self.errors),
        }
    }

    /// Convert an inferred `Type` back to an `AxonType` for the mono pass.
    /// Only handles ground types — TypeParam / Var fall back to `Named("unknown")`.
    pub fn type_to_axon(ty: &Type) -> crate::ast::AxonType {
        use crate::ast::AxonType;
        match ty {
            Type::I8 => AxonType::Named("i8".into()),
            Type::I16 => AxonType::Named("i16".into()),
            Type::I32 => AxonType::Named("i32".into()),
            Type::I64 => AxonType::Named("i64".into()),
            Type::U8 => AxonType::Named("u8".into()),
            Type::U16 => AxonType::Named("u16".into()),
            Type::U32 => AxonType::Named("u32".into()),
            Type::U64 => AxonType::Named("u64".into()),
            Type::F32 => AxonType::Named("f32".into()),
            Type::F64 => AxonType::Named("f64".into()),
            Type::Bool => AxonType::Named("bool".into()),
            Type::Str => AxonType::Named("str".into()),
            Type::Unit => AxonType::Named("unit".into()),
            Type::Struct(n) | Type::Enum(n) | Type::App(n, _) => AxonType::Named(n.clone()),
            Type::Option(inner) => AxonType::Option(Box::new(Self::type_to_axon(inner))),
            Type::Slice(inner) => AxonType::Slice(Box::new(Self::type_to_axon(inner))),
            Type::Chan(inner) => AxonType::Chan(Box::new(Self::type_to_axon(inner))),
            Type::Result(ok, err) => AxonType::Result {
                ok: Box::new(Self::type_to_axon(ok)),
                err: Box::new(Self::type_to_axon(err)),
            },
            _ => AxonType::Named("unknown".into()),
        }
    }

    /// Return all recorded call-site instantiations as (fn_name, [AxonType args]).
    /// Deduplicates identical instantiations.
    pub fn drain_instantiations(&mut self) -> Vec<(String, Vec<crate::ast::AxonType>)> {
        use std::collections::HashSet;
        let mut seen: HashSet<(String, Vec<String>)> = HashSet::new();
        let mut result = Vec::new();
        for (name, type_args) in self.call_instantiations.drain(..) {
            let axon_args: Vec<_> = type_args.iter().map(Self::type_to_axon).collect();
            let key = (
                name.clone(),
                axon_args.iter().map(|t| format!("{t:?}")).collect(),
            );
            if seen.insert(key) {
                result.push((name, axon_args));
            }
        }
        result
    }

    /// Instantiate a generic function signature for one call site.
    ///
    /// Returns a fresh `FnSig` with each `TypeParam` replaced by a new `Var`,
    /// plus the mapping from param-name → Var index (for recording the instantiation).
    fn instantiate_sig(
        &mut self,
        sig: &FnSig,
        param_names: &[String],
    ) -> (FnSig, HashMap<String, u32>) {
        let mut var_map: HashMap<String, u32> = HashMap::new();
        for name in param_names {
            let var_id = self.next_var;
            self.next_var += 1;
            var_map.insert(name.clone(), var_id);
        }
        // Substitution must be STRUCTURAL, not top-level-only: a param typed
        // `[T]`, `(str, V)`, `Option<T>` or `fn(T) -> U` carries its type
        // variable *inside* a wrapper. A top-level-only match leaves those
        // nested `TypeParam`s uninstantiated, and they then reach `unify` as
        // themselves — reported literally as `expected T`, or silently ignored.
        fn subst_ty(ty: &Type, var_map: &HashMap<String, u32>) -> Type {
            let rec = |i: &Type| Box::new(subst_ty(i, var_map));
            match ty {
                Type::TypeParam(n) => match var_map.get(n) {
                    Some(&var_id) => Type::Var(var_id),
                    None => ty.clone(),
                },
                Type::Slice(i) => Type::Slice(rec(i)),
                Type::Option(i) => Type::Option(rec(i)),
                Type::Chan(i) => Type::Chan(rec(i)),
                Type::Uncertain(i) => Type::Uncertain(rec(i)),
                Type::Temporal(i) => Type::Temporal(rec(i)),
                Type::Dict(i) => Type::Dict(rec(i)),
                Type::Result(a, b) => Type::Result(rec(a), rec(b)),
                Type::Tuple(ts) => Type::Tuple(ts.iter().map(|e| subst_ty(e, var_map)).collect()),
                Type::App(n, ts) => {
                    Type::App(n.clone(), ts.iter().map(|e| subst_ty(e, var_map)).collect())
                }
                Type::Fn(ps, r) => {
                    Type::Fn(ps.iter().map(|p| subst_ty(p, var_map)).collect(), rec(r))
                }
                other => other.clone(),
            }
        }
        let subst_ty = |ty: &Type| -> Type { subst_ty(ty, &var_map) };
        let fresh_sig = FnSig {
            params: sig.params.iter().map(&subst_ty).collect(),
            ret: subst_ty(&sig.ret),
        };
        (fresh_sig, var_map)
    }

    /// Infer `f`'s body. `top_level`: `f` is a top-level fn (not an impl
    /// method), so its bare `Dict`s hold the vars its signature owns.
    fn infer_fn(&mut self, f: &FnDef, top_level: bool) {
        // CS2-3b: the body sees the same value vars as the signature, in the
        // order `collect_sigs` gave them (params, then return).
        let owned = if top_level {
            self.dict_owned_vars.get(&f.name).cloned()
        } else {
            None
        };
        let structs = owned.as_ref().map(|_| self.implicit_only_structs());
        let mut owned = owned.unwrap_or_default().into_iter();
        let mut next = || owned.next().map(Type::Var).unwrap_or(Type::Unknown);
        let params: Vec<Type> = f
            .params
            .iter()
            .map(|p| fill_bare_dicts(self.resolve_ast_type(&p.ty), structs.as_ref(), &mut next))
            .collect();
        let ret_ty = f
            .return_type
            .as_ref()
            .map(|t| fill_bare_dicts(self.resolve_ast_type(t), structs.as_ref(), &mut next))
            .unwrap_or(Type::Unit);

        let mut scope = Scope::new();
        for (param, ty) in f.params.iter().zip(params) {
            scope.bind(param.name.clone(), ty);
        }

        // R5: #[goal] fns bind `goal_met` in their body scope.
        if f.attrs.iter().any(|a| a.name == "goal") {
            scope.bind("goal_met".to_string(), Type::I64);
        }

        let body_ty = self.infer_expr(&f.body, &mut scope, &ret_ty);
        // Constrain body type to declared return type (explicit return statements
        // also constrain individually; this handles implicit returns).
        //
        // UNLESS the body diverges. `Expr::Return` used to yield `Type::Unit`
        // (it yields `Never` since CS-11; this guard predates that), so a body
        // whose tail is a `return` was typed as `()` and
        // collided with the declared type:
        //
        //     fn g() -> i64 { return 7 }
        //     E0102 type mismatch in function body (expected i64), found ()
        //
        // `return` mid-function already worked; only the tail position failed,
        // which is the position every other language uses it in. The diagnostic
        // never said the word `return`, so a reader was told their body produced
        // `()` by a statement that plainly produces 7.
        //
        // Skipping the constraint is not skipping the check: the `Expr::Return`
        // arm constrains the returned value against `ret_ty` itself, so
        // `fn f() -> i64 { return "s" }` is still an error — by the arm that can
        // name the actual mismatch.
        if !body_diverges(&f.body) {
            self.constrain(body_ty, ret_ty, "function body");
        }
    }

    // ── Constraint solving ────────────────────────────────────────────────────

    /// Solve all accumulated constraints via union-find unification.
    pub fn solve(&mut self) -> Substitution {
        let mut subst = std::mem::take(&mut self.partial);
        let constraints = std::mem::take(&mut self.constraints);
        for c in &constraints {
            self.current_stmt_span = c.span;
            self.unify(c.lhs.clone(), c.rhs.clone(), &c.origin, &mut subst);
        }
        // CS-5: a deferred `len` arg is fine as `[T]`, else it must be `str`.
        for (ty, span) in std::mem::take(&mut self.len_obligations) {
            let ty = subst.apply(&ty);
            if !matches!(ty, Type::Slice(_)) {
                self.current_stmt_span = span;
                self.unify(ty, Type::Str, "arg 0 of `len`", &mut subst);
            }
        }
        // CS2-6: a call through a fn value checks its arity, its argument
        // kinds and its return against the callee's resolved fn type. A part
        // of that type still unresolved (an unannotated lambda param, a
        // generic) accepts anything and is never bound by the call: a lambda
        // stays callable at more than one type, as it runs.
        for ob in std::mem::take(&mut self.call_obligations) {
            let Type::Fn(params, ret) = subst.apply(&ob.callee) else {
                continue;
            };
            self.current_stmt_span = ob.span;
            if params.len() != ob.args.len() {
                let span = ob.span;
                self.errors.push(
                    InferError::new(
                        E0102,
                        format!(
                            "the fn value called here takes {} argument{} but {} {} supplied",
                            params.len(),
                            if params.len() == 1 { "" } else { "s" },
                            ob.args.len(),
                            if ob.args.len() == 1 { "was" } else { "were" },
                        ),
                    )
                    .with_span(span),
                );
                continue;
            }
            for (i, (param, arg)) in params.iter().zip(&ob.args).enumerate() {
                let arg = subst.apply(arg);
                if kinds_conflict(param, &arg) {
                    let span = ob.span;
                    self.errors.push(
                        InferError::mismatch(
                            &format!("arg {i} of a call through a fn value"),
                            param,
                            &arg,
                        )
                        .with_span(span),
                    );
                }
            }
            let ret = subst.apply(&ret);
            if !has_unresolved(&ret) {
                self.unify(
                    ob.ret,
                    ret,
                    "return of a call through a fn value",
                    &mut subst,
                );
            }
        }
        // CS2-3a: a numeric key fn (`arr_max_by`, `arr_min_by`,
        // `arr_sum_by_f64`) returns an int or a float; anything else panics
        // `key fn must return numeric` at run time. A return still unresolved
        // is left to the run time.
        for (ty, span) in std::mem::take(&mut self.num_obligations) {
            let ty = subst.apply(&ty);
            if !has_unresolved(&ty) && !ty.is_integer() && !ty.is_float() && ty != Type::Never {
                self.errors.push(
                    InferError::new(
                        E0306,
                        format!("key fn must return a number, got `{}`", ty.display()),
                    )
                    .with_span(span),
                );
            }
        }
        subst
    }

    fn unify(&mut self, lhs: Type, rhs: Type, origin: &str, subst: &mut Substitution) {
        // Apply existing substitution before comparing.
        let lhs = subst.apply(&lhs);
        let rhs = subst.apply(&rhs);

        if lhs == rhs {
            return;
        }

        match (lhs, rhs) {
            // Bind a type variable.
            (Type::Var(n), t) => {
                if self.occurs(n, &t) {
                    let span = self.current_stmt_span;
                    self.errors.push(
                        InferError::new(
                            E0102,
                            format!(
                                "cannot construct infinite type: type variable `?{n}` would \
                                 recursively contain itself in `{}`",
                                t.display()
                            ),
                        )
                        .with_span(span),
                    );
                    return;
                }
                subst.insert(n, t);
            }
            (t, Type::Var(n)) => {
                if self.occurs(n, &t) {
                    let span = self.current_stmt_span;
                    self.errors.push(
                        InferError::new(
                            E0102,
                            format!(
                                "cannot construct infinite type: type variable `?{n}` would \
                                 recursively contain itself in `{}`",
                                t.display()
                            ),
                        )
                        .with_span(span),
                    );
                    return;
                }
                subst.insert(n, t);
            }

            // Structural unification.
            (Type::Option(a), Type::Option(b)) => {
                self.unify(*a, *b, origin, subst);
            }
            (Type::Result(a1, b1), Type::Result(a2, b2)) => {
                self.unify(*a1, *a2, origin, subst);
                self.unify(*b1, *b2, origin, subst);
            }
            (Type::Slice(a), Type::Slice(b)) => {
                self.unify(*a, *b, origin, subst);
            }
            (Type::Chan(a), Type::Chan(b)) => {
                self.unify(*a, *b, origin, subst);
            }
            (Type::Uncertain(a), Type::Uncertain(b)) => {
                self.unify(*a, *b, origin, subst);
            }
            (Type::Temporal(a), Type::Temporal(b)) => {
                self.unify(*a, *b, origin, subst);
            }
            // CS2-3b: a dict holds one value type.
            (Type::Dict(a), Type::Dict(b)) => {
                self.unify(*a, *b, origin, subst);
            }
            (Type::RawPtr(a), Type::RawPtr(b)) => {
                self.unify(*a, *b, origin, subst);
            }
            // `never` unifies with anything (bottom type — can appear anywhere a type is expected).
            (Type::Never, _) | (_, Type::Never) => {}
            // Uncertain<T> is soft-compatible with T (AI soft typing: confidence
            // implicit). Temporal<T> likewise.
            //
            // The two directions are written out SEPARATELY on purpose. Both were
            // one or-pattern — `(Wrapper(inner), other) | (other, Wrapper(inner))`
            // — which binds the same names whichever side matched and so LOSES
            // the lhs/rhs orientation. Every caller passes `constrain(found,
            // expected)`, and the recursive call then handed them over swapped,
            // so a mismatch against a wrapper-typed parameter reported the
            // argument's type as "expected":
            //
            //     temporal_confidence(f)   f: Fake
            //     E0102 ... (expected Fake), found i64
            //
            // Both halves backwards: `Fake` is what was PASSED, and `i64` is the
            // parameter's inner type. Keeping the orientation makes it
            // "expected Temporal<i64>'s inner i64, found Fake" — which is the
            // way round the reader needs.
            (Type::Uncertain(inner), other) => self.unify(*inner, other, origin, subst),
            (other, Type::Uncertain(inner)) => self.unify(other, *inner, origin, subst),
            (Type::Temporal(inner), other) => self.unify(*inner, other, origin, subst),
            (other, Type::Temporal(inner)) => self.unify(other, *inner, origin, subst),
            (Type::Tuple(ts1), Type::Tuple(ts2)) if ts1.len() == ts2.len() => {
                for (a, b) in ts1.into_iter().zip(ts2) {
                    self.unify(a, b, origin, subst);
                }
            }
            (Type::Fn(ps1, r1), Type::Fn(ps2, r2)) if ps1.len() == ps2.len() => {
                for (a, b) in ps1.into_iter().zip(ps2) {
                    self.unify(a, b, origin, subst);
                }
                self.unify(*r1, *r2, origin, subst);
            }
            // CS2-5: an applied generic struct unifies by name and arguments.
            (Type::App(n1, as1), Type::App(n2, as2)) if n1 == n2 && as1.len() == as2.len() => {
                for (a, b) in as1.into_iter().zip(as2) {
                    self.unify(a, b, origin, subst);
                }
            }
            // Cutover: the bare name (`B` in a signature, no arguments) still
            // means "some `B`".
            (Type::App(n1, _), Type::Struct(n2)) | (Type::Struct(n2), Type::App(n1, _))
                if n1 == n2 => {}

            // Deferred and Unknown pass through without error.
            (Type::Deferred(_), _) | (_, Type::Deferred(_)) => {}
            (Type::Unknown, _) | (_, Type::Unknown) => {}

            // Trait object coercion: concrete struct/enum can be coerced to dyn Trait.
            (Type::Struct(_), Type::DynTrait(_)) | (Type::Enum(_), Type::DynTrait(_)) => {}
            // DynTrait on either side with Unknown/Var already handled above; skip remaining DynTrait mismatches.
            (Type::DynTrait(_), _) | (_, Type::DynTrait(_)) => {}

            // Ground type mismatch — allow implicit integer widening (i8→i16→i32→i64).
            (lhs, rhs) => {
                if !is_int_widening(&lhs, &rhs) {
                    let span = self.current_stmt_span;
                    // Convention: every `constrain(found, expected, _)` passes the
                    // value/actual type as `lhs` and the declared/expected type as
                    // `rhs`. Report in that order so "expected" is the annotation
                    // and "found" is the value — not the reverse.
                    self.errors
                        .push(InferError::mismatch(origin, &rhs, &lhs).with_span(span));
                }
            }
        }
    }

    /// Returns true if `Var(var)` appears free anywhere inside `ty`.
    fn occurs(&self, var: u32, ty: &Type) -> bool {
        match ty {
            Type::Var(n) => *n == var,
            Type::Option(inner)
            | Type::Slice(inner)
            | Type::Chan(inner)
            | Type::Uncertain(inner)
            | Type::Temporal(inner)
            | Type::RawPtr(inner)
            | Type::Dict(inner) => self.occurs(var, inner),
            Type::Result(ok, err) => self.occurs(var, ok) || self.occurs(var, err),
            Type::Tuple(ts) | Type::App(_, ts) => ts.iter().any(|t| self.occurs(var, t)),
            Type::Fn(params, ret) => {
                params.iter().any(|p| self.occurs(var, p)) || self.occurs(var, ret)
            }
            _ => false,
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BinOp as AstBinOp, Expr, Literal as AstLit, Stmt};

    fn lit_int(n: i64) -> Expr {
        Expr::Literal(AstLit::Int(n))
    }
    fn lit_str(s: &str) -> Expr {
        Expr::Literal(AstLit::Str(s.to_string()))
    }
    fn lit_bool(b: bool) -> Expr {
        Expr::Literal(AstLit::Bool(b))
    }

    fn ctx() -> (InferCtx, Scope) {
        (InferCtx::new("test"), Scope::new())
    }

    /// Type-check a whole source string, returning the inference errors.
    fn infer_errors(src: &str) -> Vec<String> {
        let program = crate::parse_source(src).expect("parses");
        let mut ctx = InferCtx::new("test.ax");
        ctx.infer_program(&program);
        ctx.errors.iter().map(|e| e.message.clone()).collect()
    }

    /// A builtin's `T`/`U`/`V` slot must behave as a type VARIABLE — bound by
    /// the call that supplies it, then checked everywhere else it appears.
    ///
    /// Regression: `parse_type_str` has no arm for a bare type variable, so
    /// these landed as `Type::Deferred`, and `unify`'s
    /// `(Deferred(_), _) | (_, Deferred(_)) => {}` arm passes `Deferred`
    /// through against ANY partner. A generic slot was therefore a permanent
    /// wildcard: it absorbed every constraint touching it. Found by an external
    /// harness, which put a value through `dict_get_or` into a struct field of
    /// an incompatible type and got no diagnostic at all — the program instead
    /// died much later, inside `arr_sum_i64`, at a call site with nothing to do
    /// A generic BUILTIN is instantiated for inference but must NOT be queued
    /// for monomorphization -- it has no AST body to specialize.
    ///
    /// Queueing one made the mono pass emit a stub that shadowed the real
    /// extern, and a native `dict_set` then silently stored nothing while the
    /// interpreter stored the value. Nothing failed to compile; the two
    /// backends just disagreed.
    #[test]
    fn a_generic_builtin_is_not_queued_for_monomorphization() {
        let program = crate::parse_source(
            "fn main() -> i64 { let d = dict_new()  dict_set(d, \"b\", 20)  dict_len(d) }",
        )
        .expect("parses");
        let mut ctx = InferCtx::new("test.ax");
        ctx.infer_program(&program);
        let queued: Vec<&str> = ctx
            .call_instantiations
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        assert!(
            !queued.contains(&"dict_set"),
            "a builtin must not reach the mono pass: {queued:?}"
        );
    }

    /// A generic USER fn named like a generic builtin, reached because the
    /// call passes `&mut` (CS-4), has a body to specialize and must still be
    /// queued: the builtin exclusion keys on the sig the call reaches, not on
    /// the name.
    #[test]
    fn a_generic_user_fn_shadowing_a_builtin_is_still_queued() {
        let program = crate::parse_source(
            "fn arr_reverse<T>(xs: &mut [T]) -> i64 { len(xs) }\n\
             fn main() -> i64 { let mut a = [1, 2]  arr_reverse(&mut a) }",
        )
        .expect("parses");
        let mut ctx = InferCtx::new("test.ax");
        ctx.infer_program(&program);
        let queued: Vec<&str> = ctx
            .call_instantiations
            .iter()
            .map(|(n, _)| n.as_str())
            .collect();
        assert!(
            queued.contains(&"arr_reverse"),
            "the user fn reached by the call must reach the mono pass: {queued:?}"
        );
    }

    /// A builtin's `V` is bound by its argument: an `i64` default makes the
    /// result `i64`, and putting it in a `[i64]` field must fail rather than
    /// be absorbed by a wildcard slot.
    #[test]
    fn builtin_type_var_return_is_bound_by_its_argument() {
        let src = r#"
type RG = { region: str, amounts: [i64] }
fn main() -> i64 {
    let d = dict_from_pairs([("n", 7)])
    let v = dict_get_or(d, "n", 0)
    let x = RG { region: "n", amounts: v }
    0
}
"#;
        let errs = infer_errors(src);
        assert!(
            errs.iter().any(|e| e.contains("struct field")),
            "an i64 flowing into a declared `[i64]` field must be caught; got {errs:?}"
        );
    }

    /// The same slot must still ACCEPT a matching type — a fix that rejects
    /// everything would pass the test above while breaking every real program.
    #[test]
    fn builtin_type_var_accepts_a_matching_argument() {
        let src = r#"
type RG = { region: str, amount: i64 }
fn main() -> i64 {
    let d = dict_from_pairs([("n", 7)])
    let v = dict_get_or(d, "n", 0)
    let x = RG { region: "n", amount: v }
    0
}
"#;
        assert!(
            infer_errors(src).is_empty(),
            "a matching i64 must type-check: {:?}",
            infer_errors(src)
        );
    }

    /// `instantiate_sig` substituted only at the TOP level, so a type variable
    /// nested inside `[T]` / `(str, V)` / `Option<T>` escaped instantiation and
    /// reached `unify` as itself — surfacing as the nonsense diagnostic
    /// "expected T, found str" on correct code. Structural substitution fixes
    /// it; this pins the shape that regressed.
    #[test]
    fn nested_type_vars_are_instantiated_not_leaked() {
        let src = r#"
fn main() -> i64 {
    let d = dict_from_pairs([("a", 1), ("b", 2)])
    let ks = dict_keys(d)
    let ups = arr_map(&ks, |k| { str_to_upper(k) })
    len(ups)
}
"#;
        let errs = infer_errors(src);
        assert!(
            errs.is_empty(),
            "nested type vars must instantiate, not leak: {errs:?}"
        );
    }

    #[test]
    fn integer_literal_infers_i64() {
        let (mut ctx, mut scope) = ctx();
        let ty = ctx.infer_expr(&lit_int(42), &mut scope, &Type::Unit);
        assert_eq!(ty, Type::I64);
        assert!(ctx.errors.is_empty());
    }

    #[test]
    fn float_literal_infers_f64() {
        let (mut ctx, mut scope) = ctx();
        let ty = ctx.infer_expr(&Expr::Literal(AstLit::Float(2.5)), &mut scope, &Type::Unit);
        assert_eq!(ty, Type::F64);
    }

    #[test]
    fn str_literal_infers_str() {
        let (mut ctx, mut scope) = ctx();
        let ty = ctx.infer_expr(&lit_str("hello"), &mut scope, &Type::Unit);
        assert_eq!(ty, Type::Str);
    }

    #[test]
    fn bool_literal_infers_bool() {
        let (mut ctx, mut scope) = ctx();
        let ty = ctx.infer_expr(&lit_bool(true), &mut scope, &Type::Unit);
        assert_eq!(ty, Type::Bool);
    }

    #[test]
    fn let_binding_propagates_type() {
        let (mut ctx, mut scope) = ctx();
        let binding = Expr::Let {
            ty: None,
            name: "x".to_string(),
            value: Box::new(lit_int(10)),
        };
        let bind_ty = ctx.infer_expr(&binding, &mut scope, &Type::Unit);
        assert_eq!(bind_ty, Type::Unit);
        // After the let, x should be i64.
        let ident_ty = ctx.infer_expr(&Expr::Ident("x".to_string()), &mut scope, &Type::Unit);
        assert_eq!(ident_ty, Type::I64);
        assert!(ctx.errors.is_empty());
    }

    #[test]
    fn binop_add_i64_i64_is_i64() {
        let (mut ctx, mut scope) = ctx();
        let add = Expr::BinOp {
            op: AstBinOp::Add,
            left: Box::new(lit_int(1)),
            right: Box::new(lit_int(2)),
        };
        let ty = ctx.infer_expr(&add, &mut scope, &Type::Unit);
        // Solve constraints.
        let subst = ctx.solve();
        let resolved = subst.apply(&ty);
        assert_eq!(resolved, Type::I64);
        assert!(ctx.errors.is_empty());
    }

    #[test]
    fn let_annotation_mismatch_reports_annotation_as_expected() {
        // Regression (BUG_HUNT #14): `let x: i64 = "string"` must report the
        // annotation (i64) as *expected* and the value (str) as *found*, not
        // the reverse.
        let (mut ctx, mut scope) = ctx();
        let binding = Expr::RefBind {
            name: "x".to_string(),
            ty: Some(AxonType::Named("i64".to_string())),
            value: Box::new(lit_str("string")),
        };
        ctx.infer_expr(&binding, &mut scope, &Type::Unit);
        ctx.solve();
        let err = ctx
            .errors
            .iter()
            .find(|e| e.code == "E0102")
            .expect("expected an E0102 mismatch");
        assert_eq!(
            err.expected.as_deref(),
            Some("i64"),
            "annotation is `expected`"
        );
        assert_eq!(err.found.as_deref(), Some("str"), "value type is `found`");
    }

    #[test]
    fn binop_add_i32_str_is_type_error() {
        let (mut ctx, mut scope) = ctx();
        let add = Expr::BinOp {
            op: AstBinOp::Add,
            left: Box::new(lit_int(1)),
            right: Box::new(lit_str("bad")),
        };
        ctx.infer_expr(&add, &mut scope, &Type::Unit);
        ctx.solve();
        // Should produce a type mismatch error.
        assert!(!ctx.errors.is_empty());
        assert_eq!(ctx.errors[0].code, "E0102");
    }

    #[test]
    fn question_unwraps_result() {
        let (mut ctx, mut scope) = ctx();
        // ok_expr has type Result<i64, _var>
        let ok_expr = Expr::Ok(Box::new(lit_int(5)));
        let question = Expr::Question(Box::new(ok_expr));
        let ret_ty = Type::Result(Box::new(Type::I64), Box::new(Type::Str));
        let ty = ctx.infer_expr(&question, &mut scope, &ret_ty);
        let subst = ctx.solve();
        let resolved = subst.apply(&ty);
        // The unwrapped type should unify to i64.
        assert!(matches!(resolved, Type::I64 | Type::Var(_)));
        assert!(ctx.errors.is_empty());
    }

    #[test]
    fn match_arms_must_agree() {
        let (mut ctx, mut scope) = ctx();
        let match_expr = Expr::Match {
            subject: Box::new(lit_bool(true)),
            arms: vec![
                crate::ast::MatchArm {
                    pattern: Pattern::Wildcard,
                    guard: None,
                    body: lit_int(1),
                },
                crate::ast::MatchArm {
                    pattern: Pattern::Wildcard,
                    guard: None,
                    body: lit_str("bad"), // mismatch
                },
            ],
        };
        ctx.infer_expr(&match_expr, &mut scope, &Type::Unit);
        ctx.solve();
        assert!(!ctx.errors.is_empty(), "should have a type mismatch error");
    }

    #[test]
    fn some_wraps_inner_type() {
        let (mut ctx, mut scope) = ctx();
        let some_expr = Expr::Some(Box::new(lit_int(42)));
        let ty = ctx.infer_expr(&some_expr, &mut scope, &Type::Unit);
        assert_eq!(ty, Type::Option(Box::new(Type::I64)));
    }

    #[test]
    fn none_gets_fresh_var_unified_by_usage() {
        let (mut ctx, mut scope) = ctx();
        // let x = None; let _y: Option<i32> = Some(1);
        // constrain x's inner var to i32 via the Some
        let none_expr = Expr::None;
        let none_ty = ctx.infer_expr(&none_expr, &mut scope, &Type::Unit);
        // none_ty should be Option<Var(n)>
        assert!(matches!(none_ty, Type::Option(_)));
        if let Type::Option(inner) = &none_ty {
            // Unify with Option<i32>
            ctx.constrain(*inner.clone(), Type::I32, "test");
        }
        let subst = ctx.solve();
        let resolved = subst.apply(&none_ty);
        assert_eq!(resolved, Type::Option(Box::new(Type::I32)));
    }

    #[test]
    fn block_type_is_last_expr() {
        let (mut ctx, mut scope) = ctx();
        let block = Expr::Block(vec![
            Stmt::simple(lit_int(1)),
            Stmt::simple(lit_bool(false)),
        ]);
        let ty = ctx.infer_expr(&block, &mut scope, &Type::Unit);
        assert_eq!(ty, Type::Bool);
    }

    #[test]
    fn builtin_sigs_registered() {
        let ctx = InferCtx::new("test");
        assert!(ctx.fn_sigs.contains_key("println"));
        assert!(ctx.fn_sigs.contains_key("assert"));
        let println_sig = &ctx.fn_sigs["println"];
        assert_eq!(println_sig.params, vec![Type::Str]);
        assert_eq!(println_sig.ret, Type::Unit);
    }

    #[test]
    fn parse_int_builtin_returns_result() {
        let ctx = InferCtx::new("test");
        let parse_int_sig = ctx.fn_sigs.get("parse_int").expect("parse_int builtin");
        assert_eq!(
            parse_int_sig.ret,
            Type::Result(Box::new(Type::I64), Box::new(Type::Str))
        );
    }

    // ── Logical operators ─────────────────────────────────────────────────────

    #[test]
    fn logical_and_infers_bool() {
        let (mut ctx, mut scope) = ctx();
        let expr = Expr::BinOp {
            op: AstBinOp::And,
            left: Box::new(lit_bool(true)),
            right: Box::new(lit_bool(false)),
        };
        let ty = ctx.infer_expr(&expr, &mut scope, &Type::Unit);
        let subst = ctx.solve();
        assert_eq!(subst.apply(&ty), Type::Bool);
        assert!(ctx.errors.is_empty());
    }

    #[test]
    fn logical_or_infers_bool() {
        let (mut ctx, mut scope) = ctx();
        let expr = Expr::BinOp {
            op: AstBinOp::Or,
            left: Box::new(lit_bool(true)),
            right: Box::new(lit_bool(true)),
        };
        let ty = ctx.infer_expr(&expr, &mut scope, &Type::Unit);
        let subst = ctx.solve();
        assert_eq!(subst.apply(&ty), Type::Bool);
        assert!(ctx.errors.is_empty());
    }

    #[test]
    fn logical_and_int_operand_is_error() {
        let (mut ctx, mut scope) = ctx();
        let expr = Expr::BinOp {
            op: AstBinOp::And,
            left: Box::new(lit_int(1)),
            right: Box::new(lit_bool(false)),
        };
        ctx.infer_expr(&expr, &mut scope, &Type::Unit);
        ctx.solve();
        assert!(!ctx.errors.is_empty(), "expected type error: i64 && bool");
    }

    #[test]
    fn modulo_infers_same_type_as_operands() {
        let (mut ctx, mut scope) = ctx();
        let expr = Expr::BinOp {
            op: AstBinOp::Rem,
            left: Box::new(lit_int(10)),
            right: Box::new(lit_int(3)),
        };
        let ty = ctx.infer_expr(&expr, &mut scope, &Type::Unit);
        let subst = ctx.solve();
        assert_eq!(subst.apply(&ty), Type::I64);
        assert!(ctx.errors.is_empty());
    }

    #[test]
    fn occurs_check_prevents_infinite_type() {
        let (mut ctx, _scope) = ctx();
        // Create a constraint that would produce an infinite type: ?0 = Option<?0>
        ctx.constrain(
            Type::Var(0),
            Type::Option(Box::new(Type::Var(0))),
            "occurs check test",
        );
        ctx.solve();
        // Should emit an infinite type error.
        assert!(!ctx.errors.is_empty(), "expected occurs check error");
    }

    // ── Integer widening ──────────────────────────────────────────────────────

    // ── Impl blocks ───────────────────────────────────────────────────────────

    #[test]
    fn impl_block_method_registered_with_mangled_name() {
        use crate::ast::{AxonType, ImplBlock, Param, Stmt};
        use crate::span::Span;
        let method = FnDef {
            public: false,
            name: "area".to_string(),
            generic_params: vec![],
            generic_bounds: vec![],
            params: vec![Param {
                name: "self_".to_string(),
                ty: AxonType::Named("Rect".to_string()),
                span: Span::dummy(),
            }],
            return_type: Some(AxonType::Named("i64".to_string())),
            body: Expr::Block(vec![Stmt::simple(lit_int(42))]),
            attrs: vec![],
            contained: None,
            verify: None,
            effect_row: None,
            span: Span::dummy(),
        };
        let program = Program {
            items: vec![Item::ImplBlock(ImplBlock {
                for_type: AxonType::Named("Rect".to_string()),
                trait_name: String::new(),
                methods: vec![method],
                generic_params: vec![],
                generic_bounds: vec![],
                span: Span::dummy(),
            })],
        };
        let mut ctx = InferCtx::new("test");
        ctx.infer_program(&program);
        // The mangled name "Rect__area" should be registered in fn_sigs.
        assert!(
            ctx.fn_sigs.contains_key("Rect__area"),
            "mangled fn sig not registered"
        );
        assert!(ctx.errors.is_empty(), "unexpected errors: {:?}", ctx.errors);
    }

    #[test]
    fn i32_to_i64_widening_does_not_produce_error() {
        let (mut ctx, _scope) = ctx();
        // Constrain i32 ~ i64 — should be allowed (widening).
        ctx.constrain(Type::I32, Type::I64, "widening test");
        ctx.solve();
        assert!(
            ctx.errors.is_empty(),
            "i32→i64 widening should not produce an inference error, got: {:?}",
            ctx.errors
        );
    }

    #[test]
    fn i64_to_i32_narrowing_produces_error() {
        let (mut ctx, _scope) = ctx();
        // Constrain i64 ~ i32 — should NOT be allowed (narrowing).
        ctx.constrain(Type::I64, Type::I32, "narrowing test");
        ctx.solve();
        assert!(
            !ctx.errors.is_empty(),
            "i64→i32 narrowing should produce an inference error"
        );
    }

    #[test]
    fn chan_unifies_with_same_inner() {
        let (mut ctx, _scope) = ctx();
        ctx.constrain(
            Type::Chan(Box::new(Type::I64)),
            Type::Chan(Box::new(Type::I64)),
            "chan unification",
        );
        ctx.solve();
        assert!(
            ctx.errors.is_empty(),
            "Chan<i64> ~ Chan<i64> should unify cleanly"
        );
    }

    #[test]
    fn chan_does_not_unify_with_different_inner() {
        let (mut ctx, _scope) = ctx();
        ctx.constrain(
            Type::Chan(Box::new(Type::I64)),
            Type::Chan(Box::new(Type::Str)),
            "chan mismatch",
        );
        ctx.solve();
        assert!(!ctx.errors.is_empty(), "Chan<i64> ~ Chan<str> should fail");
    }

    #[test]
    fn chan_resolve_from_ast_type() {
        use crate::ast::AxonType;
        let ctx = InferCtx::new("test");
        let ast_chan = AxonType::Chan(Box::new(AxonType::Named("i64".to_string())));
        let resolved = ctx.resolve_ast_type(&ast_chan);
        assert_eq!(resolved, Type::Chan(Box::new(Type::I64)));
    }

    // ── Generics ──────────────────────────────────────────────────────────────

    #[test]
    fn generic_fn_instantiates_without_error() {
        use crate::ast::{AxonType, FnDef, Item, Param, Program};
        use crate::span::Span;

        // fn identity<T>(x: T) -> T { x }
        let identity = FnDef {
            public: false,
            name: "identity".into(),
            generic_params: vec!["T".into()],
            generic_bounds: vec![],
            params: vec![Param {
                name: "x".into(),
                ty: AxonType::TypeParam("T".into()),
                span: Span::dummy(),
            }],
            return_type: Some(AxonType::TypeParam("T".into())),
            body: crate::ast::Expr::Ident("x".into()),
            attrs: vec![],
            contained: None,
            verify: None,
            effect_row: None,
            span: Span::dummy(),
        };

        // fn main() -> i64 { identity(42) }
        let call_expr = crate::ast::Expr::Call {
            callee: Box::new(crate::ast::Expr::Ident("identity".into())),
            args: vec![lit_int(42)],
            tier: None,
        };
        let main_fn = FnDef {
            public: false,
            name: "main".into(),
            generic_params: vec![],
            generic_bounds: vec![],
            params: vec![],
            return_type: Some(AxonType::Named("i64".into())),
            body: call_expr,
            attrs: vec![],
            contained: None,
            verify: None,
            effect_row: None,
            span: Span::dummy(),
        };

        let program = Program {
            items: vec![Item::FnDef(identity), Item::FnDef(main_fn)],
        };
        let mut ctx = InferCtx::new("test.ax");
        ctx.infer_program(&program);

        assert!(
            ctx.errors.is_empty(),
            "generic call should not produce errors: {:?}",
            ctx.errors
        );
        // Should have recorded one instantiation for identity.
        assert!(
            !ctx.call_instantiations.is_empty(),
            "expected call instantiation recorded"
        );
        assert_eq!(ctx.call_instantiations[0].0, "identity");
    }

    #[test]
    fn drain_instantiations_deduplicates() {
        use crate::ast::{AxonType, FnDef, Item, Param, Program};
        use crate::span::Span;

        // fn double<T>(x: T, y: T) -> T { x }
        let double_fn = FnDef {
            public: false,
            name: "double".into(),
            generic_params: vec!["T".into()],
            generic_bounds: vec![],
            params: vec![
                Param {
                    name: "x".into(),
                    ty: AxonType::TypeParam("T".into()),
                    span: Span::dummy(),
                },
                Param {
                    name: "y".into(),
                    ty: AxonType::TypeParam("T".into()),
                    span: Span::dummy(),
                },
            ],
            return_type: Some(AxonType::TypeParam("T".into())),
            body: crate::ast::Expr::Ident("x".into()),
            attrs: vec![],
            contained: None,
            verify: None,
            effect_row: None,
            span: Span::dummy(),
        };

        // fn main() { double(1, 2); double(3, 4) } — two calls, same types → one instantiation
        let body = crate::ast::Expr::Block(vec![
            Stmt::simple(crate::ast::Expr::Call {
                callee: Box::new(crate::ast::Expr::Ident("double".into())),
                args: vec![lit_int(1), lit_int(2)],
                tier: None,
            }),
            Stmt::simple(crate::ast::Expr::Call {
                callee: Box::new(crate::ast::Expr::Ident("double".into())),
                args: vec![lit_int(3), lit_int(4)],
                tier: None,
            }),
        ]);
        let main_fn = FnDef {
            public: false,
            name: "main".into(),
            generic_params: vec![],
            generic_bounds: vec![],
            params: vec![],
            return_type: Some(AxonType::Named("i64".into())),
            body,
            attrs: vec![],
            contained: None,
            verify: None,
            effect_row: None,
            span: Span::dummy(),
        };

        let program = Program {
            items: vec![Item::FnDef(double_fn), Item::FnDef(main_fn)],
        };
        let mut ctx = InferCtx::new("test.ax");
        ctx.infer_program(&program);
        assert!(
            ctx.errors.is_empty(),
            "no errors expected: {:?}",
            ctx.errors
        );

        let insts = ctx.drain_instantiations();
        assert_eq!(
            insts.len(),
            1,
            "two identical instantiations should deduplicate to one"
        );
        assert_eq!(insts[0].0, "double");
    }

    #[test]
    fn dyn_trait_method_call_infers_return_type() {
        use crate::ast::{AxonType, Param, TraitDef, TraitMethod};
        use crate::span::Span;
        // trait Greet { fn greet(self) -> str }
        let greet_trait = TraitDef {
            name: "Greet".into(),
            generic_params: vec![],
            methods: vec![TraitMethod {
                name: "greet".into(),
                params: vec![Param {
                    name: "self".into(),
                    ty: AxonType::Named("Self".into()),
                    span: Span::dummy(),
                }],
                return_type: Some(AxonType::Named("str".into())),
                span: Span::dummy(),
            }],
            span: Span::dummy(),
        };

        // fn announce(g: dyn Greet) -> unit  { g.greet() }
        let body = crate::ast::Expr::Block(vec![Stmt::simple(crate::ast::Expr::MethodCall {
            receiver: Box::new(crate::ast::Expr::Ident("g".into())),
            method: "greet".into(),
            args: vec![],
        })]);
        let announce_fn = FnDef {
            public: false,
            name: "announce".into(),
            generic_params: vec![],
            generic_bounds: vec![],
            params: vec![Param {
                name: "g".into(),
                ty: AxonType::DynTrait("Greet".into()),
                span: Span::dummy(),
            }],
            return_type: Some(AxonType::Named("str".into())),
            body,
            attrs: vec![],
            contained: None,
            verify: None,
            effect_row: None,
            span: Span::dummy(),
        };

        let program = Program {
            items: vec![Item::TraitDef(greet_trait), Item::FnDef(announce_fn)],
        };
        let mut ctx = InferCtx::new("test.ax");
        ctx.infer_program(&program);
        assert!(
            ctx.errors.is_empty(),
            "dyn Greet method call should not produce errors: {:?}",
            ctx.errors
        );
        assert!(
            ctx.trait_method_sigs.contains_key("Greet"),
            "Greet trait sigs should be registered"
        );
    }

    // ── AX-63 / AX-64 / AX-66 ─────────────────────────────────────────────────

    #[test]
    fn a_binding_nested_past_the_limit_is_refused_and_bound_unknown_ax63() {
        let slice = |t: Type| Type::Slice(Box::new(t));
        let mut ctx = InferCtx::new("test.ax");
        // The limit is wasm32-only (`MAX_TYPE_DEPTH`); native keeps none.
        assert_eq!(ctx.max_type_depth, MAX_TYPE_DEPTH);
        ctx.max_type_depth = Some(3);
        // `[[i64]]` is three levels: accepted as is.
        let ok = slice(slice(Type::I64));
        assert_eq!(ctx.refuse_deep_binding("a", ok.clone()), ok);
        assert!(ctx.errors.is_empty());
        // `[[[i64]]]` is four: E0301 at the binding, and the binding is
        // `Unknown`, so a line after it is shallow again.
        let ty = ctx.refuse_deep_binding("b", slice(slice(slice(Type::I64))));
        assert_eq!(ty, Type::Unknown);
        assert_eq!(ctx.errors.len(), 1);
        assert_eq!(ctx.errors[0].code, "E0301");
        assert_eq!(
            ctx.errors[0].message,
            "the type of `b` is nested more than 3 levels deep (the wasm32 limit)"
        );
        // No limit: nothing is refused, whatever the depth.
        ctx.max_type_depth = None;
        let deep = slice(slice(slice(slice(Type::I64))));
        assert_eq!(ctx.refuse_deep_binding("c", deep.clone()), deep);
        assert_eq!(ctx.errors.len(), 1);
    }

    #[test]
    fn a_let_chain_building_a_deep_type_is_refused_once_ax63() {
        // `let a0 = [1]`, `let a1 = [a0]`, ... : every line is shallow.
        let mut stmts = vec![Stmt::simple(Expr::Let {
            name: "a0".into(),
            ty: None,
            value: Box::new(Expr::Array(vec![lit_int(1)])),
        })];
        for i in 1..9 {
            stmts.push(Stmt::simple(Expr::Let {
                name: format!("a{i}"),
                ty: None,
                value: Box::new(Expr::Array(vec![Expr::Ident(format!("a{}", i - 1))])),
            }));
        }
        let block = Expr::Block(stmts);
        let (mut ctx, mut scope) = ctx();
        ctx.max_type_depth = Some(5);
        ctx.infer_expr(&block, &mut scope, &Type::Unit);
        // a0 = [i64] is 2 levels, a3 is 5, a4 is 6: refused. a4 is bound
        // `Unknown`, so a5..a8 are 2 to 5 levels, within the limit.
        let msgs: Vec<&str> = ctx.errors.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            msgs,
            vec!["the type of `a4` is nested more than 5 levels deep (the wasm32 limit)"],
            "{:?}",
            ctx.errors
        );
    }

    #[test]
    fn a_long_refinement_chain_resolves_without_recursion_ax66() {
        // `type A0 = i64`, `type A1 = A0`, ... : 20,000 links, each a flat line.
        // One frame per link overflowed wasm32's stack at 5,000.
        let mut ctx = InferCtx::new("test.ax");
        ctx.refinement_base
            .insert("A0".into(), AxonType::Named("i64".into()));
        for i in 1..20_000 {
            ctx.refinement_base
                .insert(format!("A{i}"), AxonType::Named(format!("A{}", i - 1)));
        }
        assert_eq!(
            ctx.resolve_ast_type(&AxonType::Named("A19999".into())),
            Type::I64
        );
        // Through a container, too.
        assert_eq!(
            ctx.resolve_ast_type(&AxonType::Slice(Box::new(AxonType::Named("A9".into())))),
            Type::Slice(Box::new(Type::I64))
        );
    }

    #[test]
    fn a_refinement_cycle_resolves_to_deferred_instead_of_overflowing_ax66() {
        let mut ctx = InferCtx::new("test.ax");
        ctx.refinement_base
            .insert("A".into(), AxonType::Named("B".into()));
        ctx.refinement_base
            .insert("B".into(), AxonType::Named("A".into()));
        ctx.refinement_base
            .insert("S".into(), AxonType::Named("S".into()));
        assert_eq!(
            ctx.resolve_ast_type(&AxonType::Named("A".into())),
            Type::Deferred("A".into())
        );
        assert_eq!(
            ctx.resolve_ast_type(&AxonType::Named("S".into())),
            Type::Deferred("S".into())
        );
    }

    #[test]
    fn a_long_binop_chain_infers_without_recursion_ax64() {
        // `1 + 1 + ... + 1`, 100,000 terms, inferred on a 4 MiB stack: one
        // `infer_expr` frame per level cannot fit (debug frames are several
        // KiB). The tree is torn down by hand: its drop glue recurses too.
        const TERMS: usize = 100_000;
        std::thread::Builder::new()
            .stack_size(4 * 1024 * 1024)
            .spawn(|| {
                let mut e = lit_int(1);
                for _ in 0..TERMS - 1 {
                    e = Expr::BinOp {
                        op: AstBinOp::Add,
                        left: Box::new(e),
                        right: Box::new(lit_int(1)),
                    };
                }
                let (mut ctx, mut scope) = ctx();
                let ty = ctx.infer_expr(&e, &mut scope, &Type::Unit);
                assert_eq!(ty, Type::I64);
                assert!(ctx.errors.is_empty(), "{:?}", ctx.errors);
                // One equality constraint per `+`, in order.
                assert_eq!(ctx.constraints.len(), TERMS - 1);
                while let Expr::BinOp { left, .. } = &mut e {
                    let next = std::mem::replace(&mut **left, Expr::None);
                    e = next;
                }
            })
            .expect("spawn")
            .join()
            .expect("a 100,000-term chain must infer on a 4 MiB stack");
    }

    #[test]
    fn a_binop_chain_keeps_its_left_to_right_inference_order_ax64() {
        // `1 + 2 + "s"`: the inner `+` constrains first (1 with 2), then the outer
        // one (the inner result with the string), as the recursive walk pushed them.
        let e = Expr::BinOp {
            op: AstBinOp::Add,
            left: Box::new(Expr::BinOp {
                op: AstBinOp::Add,
                left: Box::new(lit_int(1)),
                right: Box::new(lit_int(2)),
            }),
            right: Box::new(lit_str("s")),
        };
        let (mut ctx, mut scope) = ctx();
        let ty = ctx.infer_expr(&e, &mut scope, &Type::Unit);
        assert_eq!(ty, Type::I64);
        let origins: Vec<&str> = ctx.constraints.iter().map(|c| c.origin.as_str()).collect();
        assert_eq!(origins, vec!["arithmetic operands", "arithmetic operands"]);
        assert_eq!(
            (&ctx.constraints[0].lhs, &ctx.constraints[0].rhs),
            (&Type::I64, &Type::I64)
        );
        assert_eq!(
            (&ctx.constraints[1].lhs, &ctx.constraints[1].rhs),
            (&Type::I64, &Type::Str)
        );
    }
}
