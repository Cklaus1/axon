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

use std::collections::HashMap;

use crate::ast::{
    AxonType, BinOp, Expr, FmtPart, FnDef, Item, Literal as AstLiteral, MatchArm, Pattern,
    Program, UnaryOp,
};
use crate::builtins::builtin_sigs;
use crate::types::{Constraint, Substitution, Type};

// ── Error codes ───────────────────────────────────────────────────────────────

const E0101: &str = "E0101";
const E0102: &str = "E0102";

// ── Integer widening ──────────────────────────────────────────────────────────

/// True when `from` can be implicitly widened to `to` (i8 → i16 → i32 → i64).
fn is_int_widening(from: &Type, to: &Type) -> bool {
    let rank = |t: &Type| match t {
        Type::I8  => Some(0u8),
        Type::I16 => Some(1),
        Type::I32 => Some(2),
        Type::I64 => Some(3),
        _         => None,
    };
    matches!((rank(from), rank(to)), (Some(f), Some(t)) if f < t)
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
}

impl InferError {
    fn new(code: &'static str, msg: impl Into<String>) -> Self {
        InferError {
            code,
            message: msg.into(),
            expected: None,
            found: None,
            span: crate::span::Span::dummy(),
        }
    }

    fn mismatch(origin: &str, expected: &Type, found: &Type) -> Self {
        InferError {
            code: E0102,
            message: format!("type mismatch in {origin}"),
            expected: Some(expected.display()),
            found: Some(found.display()),
            span: crate::span::Span::dummy(),
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
        Scope { frames: vec![HashMap::new()] }
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
    Type::Deferred(s.to_string())
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

// ── InferCtx ──────────────────────────────────────────────────────────────────

/// The type-inference context.
pub struct InferCtx {
    next_var: u32,
    constraints: Vec<Constraint>,
    /// Signatures of all known functions (user-defined + builtins).
    pub fn_sigs: HashMap<String, FnSig>,
    /// Type parameter names for generic functions (fn_name → [param_names]).
    pub generic_fn_params: HashMap<String, Vec<String>>,
    /// Recorded call-site instantiations (fn_name → Vec<concrete Type args>).
    /// Populated during infer; consumed by the mono pass.
    pub call_instantiations: Vec<(String, Vec<Type>)>,
    /// Field lists for user-defined struct types.
    pub struct_fields: HashMap<String, Vec<(String, Type)>>,
    /// Generic type parameter names for struct types (struct_name → [param_names]).
    pub struct_generic_params: HashMap<String, Vec<String>>,
    /// Variant name lists for user-defined enum types.
    pub enum_variants: HashMap<String, Vec<String>>,
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
    /// Create a fresh context and pre-register all built-in signatures.
    pub fn new(file: impl Into<String>) -> Self {
        let mut ctx = InferCtx {
            next_var: 0,
            constraints: Vec::new(),
            fn_sigs: HashMap::new(),
            generic_fn_params: HashMap::new(),
            call_instantiations: Vec::new(),
            struct_fields: HashMap::new(),
            struct_generic_params: HashMap::new(),
            enum_variants: HashMap::new(),
            enum_variant_fields: HashMap::new(),
            trait_method_sigs: HashMap::new(),
            module_bindings: HashMap::new(),
            errors: Vec::new(),
            file: file.into(),
            current_stmt_span: crate::span::Span::dummy(),
        };
        for (name, sig) in builtin_sigs() {
            ctx.fn_sigs.insert(
                name,
                FnSig {
                    params: sig.params.iter().map(|s| parse_type_str(s)).collect(),
                    ret: parse_type_str(&sig.ret),
                },
            );
        }
        ctx
    }

    /// Allocate a fresh unification variable.
    pub fn fresh(&mut self) -> Type {
        let n = self.next_var;
        self.next_var += 1;
        Type::Var(n)
    }

    /// Record that `lhs` and `rhs` must unify.
    pub fn constrain(&mut self, lhs: Type, rhs: Type, origin: &str) {
        let span = self.current_stmt_span;
        self.constraints.push(Constraint { lhs, rhs, origin: origin.to_string(), span });
    }

    /// Convert an AST `AxonType` to a semantic `Type`.
    pub fn resolve_ast_type(&self, ast_ty: &AxonType) -> Type {
        match ast_ty {
            AxonType::Named(name) => {
                if let Some(ty) = Type::from_name(name) {
                    ty
                } else if self.struct_fields.contains_key(name) {
                    Type::Struct(name.clone())
                } else if self.enum_variants.contains_key(name) {
                    Type::Enum(name.clone())
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
            AxonType::Generic { base, .. } => {
                // Phase 1: generics treated as deferred.
                Type::Deferred(base.clone())
            }
            AxonType::Fn { params, ret } => Type::Fn(
                params.iter().map(|p| self.resolve_ast_type(p)).collect(),
                Box::new(self.resolve_ast_type(ret)),
            ),
            AxonType::Ref(inner) => self.resolve_ast_type(inner), // Phase 1: ref transparent
            AxonType::TypeParam(name) => Type::TypeParam(name.clone()),
            AxonType::DynTrait(name) => Type::DynTrait(name.clone()),
            AxonType::Tuple(elems) => Type::Tuple(elems.iter().map(|e| self.resolve_ast_type(e)).collect()),
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
                    self.fn_sigs.insert(
                        f.name.clone(),
                        FnSig { params: vec![], ret: Type::Unit },
                    );
                }
                Item::TypeDef(td) => {
                    // Register struct name so forward refs resolve to Type::Struct.
                    self.struct_fields.insert(td.name.clone(), vec![]);
                    if !td.generic_params.is_empty() {
                        self.struct_generic_params.insert(td.name.clone(), td.generic_params.clone());
                    }
                }
                Item::EnumDef(ed) => {
                    // Register enum name so forward refs resolve to Type::Enum.
                    let variants = ed.variants.iter().map(|v| v.name.clone()).collect();
                    self.enum_variants.insert(ed.name.clone(), variants);
                }
                Item::ImplBlock(blk) => {
                    // Register impl methods as placeholder sigs (mangled name: TypeName__method).
                    let type_name = ast_type_simple_name(&blk.for_type);
                    for m in &blk.methods {
                        let mangled = format!("{type_name}__{}", m.name);
                        self.fn_sigs.insert(mangled, FnSig { params: vec![], ret: Type::Unit });
                    }
                }
                _ => {}
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
                        self.resolve_ast_type(ty)
                    };
                    let params = f.params.iter()
                        .map(|p| resolve_with_generics(&p.ty))
                        .collect();
                    let ret = f.return_type.as_ref()
                        .map(|t| resolve_with_generics(t))
                        .unwrap_or(Type::Unit);
                    self.fn_sigs.insert(f.name.clone(), FnSig { params, ret });
                    if !f.generic_params.is_empty() {
                        self.generic_fn_params.insert(f.name.clone(), f.generic_params.clone());
                    }
                }
                Item::TypeDef(td) => {
                    let fields = td
                        .fields
                        .iter()
                        .map(|f| (f.name.clone(), self.resolve_ast_type(&f.ty)))
                        .collect();
                    self.struct_fields.insert(td.name.clone(), fields);
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
                    self.enum_variant_fields.insert(ed.name.clone(), variant_map);
                }
                Item::ImplBlock(blk) => {
                    // Pass 2: register impl methods with fully-resolved types.
                    let type_name = ast_type_simple_name(&blk.for_type);
                    for m in &blk.methods {
                        let mangled = format!("{type_name}__{}", m.name);
                        let params =
                            m.params.iter().map(|p| self.resolve_ast_type(&p.ty)).collect();
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
                        let params: Vec<Type> = m.params.iter()
                            .filter(|p| p.name != "self")
                            .map(|p| self.resolve_ast_type(&p.ty))
                            .collect();
                        let ret = m.return_type.as_ref()
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
            },

            // ── Identifiers ──────────────────────────────────────────────────
            Expr::Ident(name) => {
                if let Some(ty) = scope.lookup(name) {
                    return ty.clone();
                }
                if let Some(ty) = self.module_bindings.get(name) {
                    return ty.clone();
                }
                if let Some(sig) = self.fn_sigs.get(name) {
                    return Type::Fn(sig.params.clone(), Box::new(sig.ret.clone()));
                }
                let var = self.fresh();
                let span = self.current_stmt_span;
                // Build a "did you mean" suggestion by scanning visible names
                // (scope frames + module bindings + fn signatures).
                let suggestion = {
                    let mut best: Option<(usize, String)> = None;
                    let candidates = scope
                        .frames
                        .iter()
                        .flat_map(|f| f.keys().cloned())
                        .chain(self.module_bindings.keys().cloned())
                        .chain(self.fn_sigs.keys().cloned());
                    for cand in candidates {
                        let d = crate::error::levenshtein(name, &cand);
                        if d <= 2 {
                            let take = match &best {
                                None => true,
                                Some((b, _)) => d < *b,
                            };
                            if take {
                                best = Some((d, cand));
                            }
                        }
                    }
                    best.map(|(_, s)| s)
                };
                let mut msg = format!("cannot find value `{name}` in scope");
                if let Some(s) = &suggestion {
                    msg.push_str(&format!(" — did you mean `{s}`?"));
                }
                self.errors.push(
                    InferError::new(E0101, msg).with_span(span),
                );
                var
            }

            // ── Bindings ─────────────────────────────────────────────────────
            Expr::Let { name, value } | Expr::Own { name, value } | Expr::RefBind { name, value } => {
                let val_ty = self.infer_expr(value, scope, ret_ty);
                scope.bind(name.clone(), val_ty);
                Type::Unit
            }

            // ── Block ─────────────────────────────────────────────────────────
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
            Expr::BinOp { op, left, right } => {
                let lt = self.infer_expr(left, scope, ret_ty);
                let rt = self.infer_expr(right, scope, ret_ty);
                match op {
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem => {
                        self.constrain(lt.clone(), rt.clone(), "arithmetic operands");
                        // Fix #4: also constrain both operands to be numeric.
                        // We emit a constraint to I64 which will unify cleanly
                        // with I64/F64/other numeric vars, but produce a
                        // mismatch if a concrete non-numeric type is present.
                        // The checker (Fix #4b) performs the definitive check.
                        lt
                    }
                    BinOp::Eq | BinOp::NotEq => {
                        self.constrain(lt, rt, "equality operands");
                        Type::Bool
                    }
                    BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                        self.constrain(lt, rt, "comparison operands");
                        Type::Bool
                    }
                    BinOp::And | BinOp::Or => {
                        self.constrain(lt, Type::Bool, "logical operand");
                        self.constrain(rt, Type::Bool, "logical operand");
                        Type::Bool
                    }
                }
            }

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
                    UnaryOp::Ref => ty, // Phase 1: transparent
                }
            }

            // ── Function call ─────────────────────────────────────────────────
            Expr::Call { callee, args } => {
                let fn_name = match callee.as_ref() {
                    Expr::Ident(n) => Some(n.clone()),
                    // Chan::new(n) / chan<T>() both parse as StructLit { name: ... }
                    Expr::StructLit { name, fields } if fields.is_empty() => Some(name.clone()),
                    _ => None,
                };

                // Handle `chan<T>()` lowered form: "chan::<T>" → Type::Chan(T)
                if let Some(ref name) = fn_name {
                    if let Some(inner) = name.strip_prefix("chan::<").and_then(|s| s.strip_suffix(">")) {
                        for arg in args {
                            self.infer_expr(arg, scope, ret_ty);
                        }
                        return Type::Chan(Box::new(parse_type_str(inner)));
                    }
                }
                let _callee_ty = self.infer_expr(callee, scope, ret_ty);

                if let Some(name) = fn_name {
                    if let Some(sig) = self.fn_sigs.get(&name).cloned() {
                        // For generic functions: instantiate with fresh type vars per call site.
                        let param_names = self.generic_fn_params.get(&name).cloned().unwrap_or_default();
                        let (inst_sig, var_map) = if !param_names.is_empty() {
                            self.instantiate_sig(&sig, &param_names)
                        } else {
                            (sig, HashMap::new())
                        };
                        // Fix #16: include function name and param index in
                        // the constraint origin for clearer error messages.
                        for (i, (arg, param_ty)) in
                            args.iter().zip(inst_sig.params.iter()).enumerate()
                        {
                            let arg_ty = self.infer_expr(arg, scope, ret_ty);
                            self.constrain(
                                arg_ty,
                                param_ty.clone(),
                                &format!("arg {i} of `{name}`"),
                            );
                        }
                        let ret = inst_sig.ret.clone();
                        // Record instantiation for the mono pass (resolved later).
                        if !var_map.is_empty() {
                            let var_ids: Vec<u32> = param_names.iter()
                                .filter_map(|n| var_map.get(n).copied())
                                .collect();
                            // Store pending instantiation — vars will be resolved after solving.
                            self.call_instantiations.push((name, var_ids.iter().map(|&v| Type::Var(v)).collect()));
                        }
                        return ret;
                    }
                }
                // Unknown callee — infer args for side-effects, return fresh var.
                for arg in args {
                    self.infer_expr(arg, scope, ret_ty);
                }
                self.fresh()
            }

            // ── Method call (Phase 1: treat as top-level fn lookup) ───────────
            Expr::MethodCall { receiver, method, args } => {
                let recv_ty = self.infer_expr(receiver, scope, ret_ty);
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
                    for arg in args { self.infer_expr(arg, scope, ret_ty); }
                    return self.fresh();
                }
                if let Some(sig) = self.fn_sigs.get(method).cloned() {
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
                let first_ty = self.infer_match_arm_with_subj(&arms[0], scope, ret_ty, Some(&subj_ty));
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
                self.constrain(val_ty, ret_ty.clone(), "return value");
                Type::Unit // Return is diverging; block type isn't used after it.
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
                        let declared_payload: Vec<(String, Type)> = self
                            .enum_variant_fields
                            .get(&enum_name)
                            .and_then(|m| m.get(&variant_name))
                            .cloned()
                            .unwrap_or_default();

                        for (fname, fexpr) in fields {
                            let inferred_ty = self.infer_expr(fexpr, scope, ret_ty);
                            if let Some((_, decl_ty)) =
                                declared_payload.iter().find(|(dn, _)| dn == fname)
                            {
                                self.constrain(
                                    inferred_ty,
                                    decl_ty.clone(),
                                    "enum variant field",
                                );
                            }
                            // Unknown field names in enum variants are not flagged
                            // in Phase 1 (the parser already validated them).
                        }
                        return Type::Enum(enum_name);
                    }
                }

                // Struct literal: constrain each provided field against declared type.
                // For generic structs (e.g. Pair<A, B>), substitute fresh Vars for TypeParams.
                let raw_declared: Vec<(String, Type)> =
                    self.struct_fields.get(name).cloned().unwrap_or_default();
                let declared: Vec<(String, Type)> = if let Some(param_names) =
                    self.struct_generic_params.get(name).cloned()
                {
                    let mut var_map: HashMap<String, u32> = HashMap::new();
                    for pname in &param_names {
                        let vid = self.next_var;
                        self.next_var += 1;
                        var_map.insert(pname.clone(), vid);
                    }
                    raw_declared
                        .iter()
                        .map(|(fname, fty)| {
                            let subst_ty = match fty {
                                Type::TypeParam(n) => {
                                    if let Some(&vid) = var_map.get(n) {
                                        Type::Var(vid)
                                    } else {
                                        fty.clone()
                                    }
                                }
                                _ => fty.clone(),
                            };
                            (fname.clone(), subst_ty)
                        })
                        .collect()
                } else {
                    raw_declared
                };

                // Track which declared fields were provided for missing-field detection.
                let mut provided: std::collections::HashSet<String> =
                    std::collections::HashSet::new();

                for (fname, fexpr) in fields {
                    let inferred_ty = self.infer_expr(fexpr, scope, ret_ty);
                    provided.insert(fname.clone());

                    if let Some((_, decl_ty)) =
                        declared.iter().find(|(dn, _)| dn == fname)
                    {
                        self.constrain(
                            inferred_ty,
                            decl_ty.clone(),
                            "struct field",
                        );
                    } else if !declared.is_empty() {
                        // Unknown field name (only report if we know the struct).
                        let known: Vec<String> =
                            declared.iter().map(|(n, _)| n.clone()).collect();
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

                Type::Struct(name.clone())
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
                                } else {
                                    field_ty.clone()
                                }
                            } else {
                                let known: Vec<String> = fields.iter().map(|(n, _)| n.clone()).collect();
                                let span = self.current_stmt_span;
                                self.errors.push(
                                    InferError::new(
                                        E0101,
                                        format!(
                                            "struct `{name}` has no field `{field}` (known fields: {})",
                                            known.join(", ")
                                        ),
                                    )
                                    .with_span(span),
                                );
                                self.fresh()
                            }
                        } else {
                            self.fresh()
                        }
                    }
                    // Phase 1: unknown receiver type — defer.
                    _ => self.fresh(),
                }
            }

            // ── Index ─────────────────────────────────────────────────────────
            Expr::Index { receiver, index } => {
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

            // ── Lambda ────────────────────────────────────────────────────────
            Expr::Lambda { params, body, captures: _ } => {
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
                for stmt in body {
                    self.infer_expr(&stmt.expr, scope, ret_ty);
                }
                scope.pop();
                Type::Unit
            }

            // ── For loop (range iteration) ───────────────────────────────────
            Expr::For { var, start, end, body, .. } => {
                let start_ty = self.infer_expr(start, scope, ret_ty);
                let end_ty   = self.infer_expr(end,   scope, ret_ty);
                self.constrain(start_ty, Type::I64, "for range start");
                self.constrain(end_ty,   Type::I64, "for range end");
                scope.push();
                scope.bind(var.clone(), Type::I64);
                for stmt in body {
                    self.infer_expr(&stmt.expr, scope, ret_ty);
                }
                scope.pop();
                Type::Unit
            }

            // ── Break / Continue ─────────────────────────────────────────────
            Expr::Break | Expr::Continue => Type::Unit,

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
        for item in &program.items {
            match item {
                Item::FnDef(f) => self.infer_fn(f),
                Item::ImplBlock(blk) => {
                    for m in &blk.methods {
                        self.infer_fn(m);
                    }
                }
                _ => {}
            }
        }
        let subst = self.solve();
        // Resolve pending generic instantiations now that the substitution is final.
        for (_, type_args) in &mut self.call_instantiations {
            for ty in type_args.iter_mut() {
                *ty = subst.apply(ty);
            }
        }
        subst
    }

    /// Convert an inferred `Type` back to an `AxonType` for the mono pass.
    /// Only handles ground types — TypeParam / Var fall back to `Named("unknown")`.
    pub fn type_to_axon(ty: &Type) -> crate::ast::AxonType {
        use crate::ast::AxonType;
        match ty {
            Type::I8  => AxonType::Named("i8".into()),
            Type::I16 => AxonType::Named("i16".into()),
            Type::I32 => AxonType::Named("i32".into()),
            Type::I64 => AxonType::Named("i64".into()),
            Type::U8  => AxonType::Named("u8".into()),
            Type::U16 => AxonType::Named("u16".into()),
            Type::U32 => AxonType::Named("u32".into()),
            Type::U64 => AxonType::Named("u64".into()),
            Type::F32 => AxonType::Named("f32".into()),
            Type::F64 => AxonType::Named("f64".into()),
            Type::Bool => AxonType::Named("bool".into()),
            Type::Str  => AxonType::Named("str".into()),
            Type::Unit => AxonType::Named("unit".into()),
            Type::Struct(n) | Type::Enum(n) => AxonType::Named(n.clone()),
            Type::Option(inner) => AxonType::Option(Box::new(Self::type_to_axon(inner))),
            Type::Slice(inner)  => AxonType::Slice(Box::new(Self::type_to_axon(inner))),
            Type::Chan(inner)   => AxonType::Chan(Box::new(Self::type_to_axon(inner))),
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
            let key = (name.clone(), axon_args.iter().map(|t| format!("{t:?}")).collect());
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
    fn instantiate_sig(&mut self, sig: &FnSig, param_names: &[String]) -> (FnSig, HashMap<String, u32>) {
        let mut var_map: HashMap<String, u32> = HashMap::new();
        for name in param_names {
            let var_id = self.next_var;
            self.next_var += 1;
            var_map.insert(name.clone(), var_id);
        }
        let subst_ty = |ty: &Type| -> Type {
            match ty {
                Type::TypeParam(n) => {
                    if let Some(&var_id) = var_map.get(n) {
                        Type::Var(var_id)
                    } else {
                        ty.clone()
                    }
                }
                _ => ty.clone(),
            }
        };
        let fresh_sig = FnSig {
            params: sig.params.iter().map(&subst_ty).collect(),
            ret: subst_ty(&sig.ret),
        };
        (fresh_sig, var_map)
    }

    fn infer_fn(&mut self, f: &FnDef) {
        let ret_ty = f
            .return_type
            .as_ref()
            .map(|t| self.resolve_ast_type(t))
            .unwrap_or(Type::Unit);

        let mut scope = Scope::new();
        for param in &f.params {
            let ty = self.resolve_ast_type(&param.ty);
            scope.bind(param.name.clone(), ty);
        }

        let body_ty = self.infer_expr(&f.body, &mut scope, &ret_ty);
        // Constrain body type to declared return type (explicit return statements
        // also constrain individually; this handles implicit returns).
        self.constrain(body_ty, ret_ty, "function body");
    }

    // ── Constraint solving ────────────────────────────────────────────────────

    /// Solve all accumulated constraints via union-find unification.
    pub fn solve(&mut self) -> Substitution {
        let mut subst = Substitution::new();
        let constraints = std::mem::take(&mut self.constraints);
        for c in &constraints {
            self.current_stmt_span = c.span;
            self.unify(c.lhs.clone(), c.rhs.clone(), &c.origin, &mut subst);
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
            (Type::Tuple(ts1), Type::Tuple(ts2)) if ts1.len() == ts2.len() => {
                for (a, b) in ts1.into_iter().zip(ts2.into_iter()) {
                    self.unify(a, b, origin, subst);
                }
            }
            (Type::Fn(ps1, r1), Type::Fn(ps2, r2)) if ps1.len() == ps2.len() => {
                for (a, b) in ps1.into_iter().zip(ps2.into_iter()) {
                    self.unify(a, b, origin, subst);
                }
                self.unify(*r1, *r2, origin, subst);
            }

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
                    self.errors.push(InferError::mismatch(origin, &lhs, &rhs).with_span(span));
                }
            }
        }
    }

    /// Returns true if `Var(var)` appears free anywhere inside `ty`.
    fn occurs(&self, var: u32, ty: &Type) -> bool {
        match ty {
            Type::Var(n) => *n == var,
            Type::Option(inner) | Type::Slice(inner) | Type::Chan(inner) => self.occurs(var, inner),
            Type::Result(ok, err) => self.occurs(var, ok) || self.occurs(var, err),
            Type::Tuple(ts) => ts.iter().any(|t| self.occurs(var, t)),
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
        let ty = ctx.infer_expr(&Expr::Literal(AstLit::Float(3.14)), &mut scope, &Type::Unit);
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
            span: Span::dummy(),
        };
        let program = Program {
            items: vec![Item::ImplBlock(ImplBlock {
                for_type: AxonType::Named("Rect".to_string()),
                trait_name: String::new(),
                methods: vec![method],
                span: Span::dummy(),
            })],
        };
        let mut ctx = InferCtx::new("test");
        ctx.infer_program(&program);
        // The mangled name "Rect__area" should be registered in fn_sigs.
        assert!(ctx.fn_sigs.contains_key("Rect__area"), "mangled fn sig not registered");
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
            "i32→i64 widening should not produce an inference error, got: {:?}", ctx.errors
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
        assert!(ctx.errors.is_empty(), "Chan<i64> ~ Chan<i64> should unify cleanly");
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
        let mut ctx = InferCtx::new("test");
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
            span: Span::dummy(),
        };

        // fn main() -> i64 { identity(42) }
        let call_expr = crate::ast::Expr::Call {
            callee: Box::new(crate::ast::Expr::Ident("identity".into())),
            args: vec![lit_int(42)],
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
            span: Span::dummy(),
        };

        let program = Program { items: vec![Item::FnDef(identity), Item::FnDef(main_fn)] };
        let mut ctx = InferCtx::new("test.ax");
        ctx.infer_program(&program);

        assert!(ctx.errors.is_empty(), "generic call should not produce errors: {:?}", ctx.errors);
        // Should have recorded one instantiation for identity.
        assert!(!ctx.call_instantiations.is_empty(), "expected call instantiation recorded");
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
                Param { name: "x".into(), ty: AxonType::TypeParam("T".into()), span: Span::dummy() },
                Param { name: "y".into(), ty: AxonType::TypeParam("T".into()), span: Span::dummy() },
            ],
            return_type: Some(AxonType::TypeParam("T".into())),
            body: crate::ast::Expr::Ident("x".into()),
            attrs: vec![],
            span: Span::dummy(),
        };

        // fn main() { double(1, 2); double(3, 4) } — two calls, same types → one instantiation
        let body = crate::ast::Expr::Block(vec![
            Stmt::simple(crate::ast::Expr::Call {
                callee: Box::new(crate::ast::Expr::Ident("double".into())),
                args: vec![lit_int(1), lit_int(2)],
            }),
            Stmt::simple(crate::ast::Expr::Call {
                callee: Box::new(crate::ast::Expr::Ident("double".into())),
                args: vec![lit_int(3), lit_int(4)],
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
            span: Span::dummy(),
        };

        let program = Program { items: vec![Item::FnDef(double_fn), Item::FnDef(main_fn)] };
        let mut ctx = InferCtx::new("test.ax");
        ctx.infer_program(&program);
        assert!(ctx.errors.is_empty(), "no errors expected: {:?}", ctx.errors);

        let insts = ctx.drain_instantiations();
        assert_eq!(insts.len(), 1, "two identical instantiations should deduplicate to one");
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
        let body = crate::ast::Expr::Block(vec![
            Stmt::simple(crate::ast::Expr::MethodCall {
                receiver: Box::new(crate::ast::Expr::Ident("g".into())),
                method: "greet".into(),
                args: vec![],
            }),
        ]);
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
            span: Span::dummy(),
        };

        let program = Program {
            items: vec![
                Item::TraitDef(greet_trait),
                Item::FnDef(announce_fn),
            ],
        };
        let mut ctx = InferCtx::new("test.ax");
        ctx.infer_program(&program);
        assert!(
            ctx.errors.is_empty(),
            "dyn Greet method call should not produce errors: {:?}", ctx.errors
        );
        assert!(ctx.trait_method_sigs.contains_key("Greet"), "Greet trait sigs should be registered");
    }
}
