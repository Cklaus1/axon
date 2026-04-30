//! Type checker for Axon — Phase 1 (12 rules).
//!
//! This module runs after type inference has resolved the type of every
//! expression.  It collects *all* errors rather than stopping at the first
//! one.
//!
//! ## Rules implemented
//! R01 E0301 – Option<T> used without unwrapping
//! R02 E0302 – Result<T,E> return value ignored
//! R03 E0303 – `?` operator inside a non-Result function
//! R04 E0304 – non-exhaustive match on Option / Result
//! R05 E0305 – wrong argument count
//! R06 E0306 – wrong argument type
//! R07 E0307 – return type mismatch
//! R08 E0308 – unknown type annotation
//! R11 E0309 – field access on non-struct / missing field
//! R12        – Deferred types are transparent (no error)

use std::collections::{HashMap, HashSet};

use crate::ast::{
    AxonType, Expr, FmtPart, FnDef, Item, MatchArm, Pattern, Program, Stmt,
};
use crate::error::levenshtein;
use crate::types::Type;

// ── Error codes ───────────────────────────────────────────────────────────────

/// Type mismatch (shared with inference pass).
pub const E0102: &str = "E0102";
pub const E0301: &str = "E0301";
pub const E0302: &str = "E0302";
pub const E0303: &str = "E0303";
pub const E0304: &str = "E0304";
pub const E0305: &str = "E0305";
pub const E0306: &str = "E0306";
pub const E0307: &str = "E0307";
pub const E0308: &str = "E0308";
pub const E0309: &str = "E0309";
pub const E0401: &str = "E0401"; // struct has no field (Phase 3 canonical code)

// Trait validation error codes (Phase 3+)
pub const E0501: &str = "E0501"; // impl block names a trait that does not exist
pub const E0502: &str = "E0502"; // impl block is missing required trait methods
pub const E0503: &str = "E0503"; // impl method signature differs from trait declaration
pub const E0504: &str = "E0504"; // trait bound not satisfied: type does not implement trait

// ── Severity ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

// ── CheckError ────────────────────────────────────────────────────────────────

/// A diagnostic produced by the type checker.
/// This mirrors the `AxonError` struct described in `error.rs` (which is a
/// stub while the two modules are developed in parallel).
#[derive(Debug, Clone)]
pub struct CheckError {
    pub code: &'static str,
    pub message: String,
    pub node_id: String,
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub expected: Option<String>,
    pub found: Option<String>,
    pub fix: Option<String>,
    pub severity: Severity,
    pub span: crate::span::Span,
}

impl CheckError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            node_id: String::new(),
            file: String::new(),
            line: 0,
            col: 0,
            expected: Option::None,
            found: Option::None,
            fix: Option::None,
            severity: Severity::Error,
            span: crate::span::Span::dummy(),
        }
    }

    pub fn at(mut self, file: impl Into<String>, line: u32, col: u32) -> Self {
        self.file = file.into();
        self.line = line;
        self.col = col;
        self
    }

    pub fn with_span(mut self, span: crate::span::Span) -> Self {
        self.span = span;
        self
    }

    pub fn node(mut self, id: impl Into<String>) -> Self {
        self.node_id = id.into();
        self
    }

    pub fn expected(mut self, e: impl Into<String>) -> Self {
        self.expected = Option::Some(e.into());
        self
    }

    pub fn found(mut self, f: impl Into<String>) -> Self {
        self.found = Option::Some(f.into());
        self
    }

    pub fn fix(mut self, f: impl Into<String>) -> Self {
        self.fix = Option::Some(f.into());
        self
    }
}

// ── Integer widening (implicit coercion at call sites) ────────────────────────

/// Returns true if `from` can be implicitly widened to `to` at a call site.
/// Only signed-integer widening is allowed (i8→i16→i32→i64); no float or
/// cross-kind widening.  This makes `to_str(abs_i32(-5))` valid at the
/// language level, matching what codegen already emits via sext.
/// Returns true if `ty` recursively contains an unresolved type parameter or Unknown.
/// Used to suppress false-positive E0306 for generic callers (e.g. `is_none(None)`).
fn type_contains_unresolved(ty: &Type) -> bool {
    match ty {
        Type::TypeParam(_) | Type::Unknown | Type::Var(_) | Type::Deferred(_) => true,
        // Uncertain<T> and Temporal<T> are AI-typed — suppress false-positive E0306.
        Type::Uncertain(_) | Type::Temporal(_) => true,
        Type::Option(inner) | Type::Slice(inner) | Type::Chan(inner) => {
            type_contains_unresolved(inner)
        }
        Type::Result(ok, err) => {
            type_contains_unresolved(ok) || type_contains_unresolved(err)
        }
        _ => false,
    }
}

fn is_integer_widening(from: &Type, to: &Type) -> bool {
    let rank = |t: &Type| match t {
        Type::I8  => Some(0u8),
        Type::I16 => Some(1),
        Type::I32 => Some(2),
        Type::I64 => Some(3),
        _         => None,
    };
    matches!((rank(from), rank(to)), (Some(f), Some(t)) if f < t)
}

// ── Known primitives (for R08) ────────────────────────────────────────────────

const PRIMITIVE_NAMES: &[&str] = &[
    "i8", "i16", "i32", "i64",
    "u8", "u16", "u32", "u64",
    "f32", "f64",
    "bool", "str", "String",
    "()", "unit",
];

/// Deferred type name prefixes (R08 / R12): always valid, never emit E0308.
const DEFERRED_PREFIXES: &[&str] = &["Uncertain", "Temporal", "Goal"];

fn is_known_type_name(
    name: &str,
    struct_fields: &HashMap<String, Vec<(String, Type)>>,
    enum_names: &[String],
) -> bool {
    if PRIMITIVE_NAMES.contains(&name) {
        return true;
    }
    if DEFERRED_PREFIXES.iter().any(|p| name.starts_with(p)) {
        return true;
    }
    if struct_fields.contains_key(name) {
        return true;
    }
    if enum_names.iter().any(|e| e == name) {
        return true;
    }
    false
}

// ── Function signature ────────────────────────────────────────────────────────

/// Resolved signature of a function (populated by the inference phase and
/// passed into the checker).
#[derive(Debug, Clone)]
pub struct FnSig {
    pub params: Vec<Type>,
    pub ret: Type,
}

// ── CheckCtx ─────────────────────────────────────────────────────────────────

pub struct CheckCtx {
    pub file: String,
    pub fn_sigs: HashMap<String, FnSig>,
    pub struct_fields: HashMap<String, Vec<(String, Type)>>,
    /// Resolved type for each expression node, keyed by node-path string.
    /// Populated via `check_program` just before checking starts.
    pub expr_types: HashMap<String, Type>,
    pub errors: Vec<CheckError>,
    /// Current function's declared return type (set during `check_fn`).
    current_ret_ty: Option<Type>,
    /// Enum names collected from the program for R08 resolution.
    known_enums: Vec<String>,
    /// Variant lists for user-defined enums — used by Fix #10 for exhaustiveness.
    pub enum_variants: HashMap<String, Vec<String>>,
    /// Generic type parameter names in scope for the current function (R08 suppression).
    current_generic_params: HashSet<String>,
    /// Trait definitions collected from the program: trait_name → TraitDef.
    /// Populated during `check_program` for E0501/E0502/E0503 validation.
    trait_defs: HashMap<String, crate::ast::TraitDef>,
    /// Impl table: concrete type name → set of trait names it implements.
    /// Built from `ImplBlock` items during `check_program` for E0504.
    impl_table: HashMap<String, HashSet<String>>,
    /// Per-function generic bounds: fn_name → Vec<(param_name, trait_names)>.
    /// Built from `FnDef.generic_bounds` during `check_program` for E0504.
    fn_bounds: HashMap<String, Vec<(String, Vec<String>)>>,
    /// Span of the statement (or sub-expression) currently being checked.
    /// Updated at every `Stmt` boundary inside `check_expr` so that diagnostics
    /// emitted from deeper visits carry source-location info even when the
    /// individual `Expr` variant has no span field of its own.
    current_span: crate::span::Span,
}

impl CheckCtx {
    pub fn new(
        file: impl Into<String>,
        fn_sigs: HashMap<String, FnSig>,
        struct_fields: HashMap<String, Vec<(String, Type)>>,
    ) -> Self {
        Self {
            file: file.into(),
            fn_sigs,
            struct_fields,
            expr_types: HashMap::new(),
            errors: Vec::new(),
            current_ret_ty: Option::None,
            known_enums: Vec::new(),
            enum_variants: HashMap::new(),
            current_generic_params: HashSet::new(),
            trait_defs: HashMap::new(),
            impl_table: HashMap::new(),
            fn_bounds: HashMap::new(),
            current_span: crate::span::Span::dummy(),
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Public entry point
    // ─────────────────────────────────────────────────────────────────────────

    /// Run all checks on the program and return every collected error.
    ///
    /// `expr_types` maps node-path strings to the resolved `Type` for each
    /// expression, as produced by the inference phase.
    pub fn check_program(
        &mut self,
        program: &Program,
        expr_types: HashMap<String, Type>,
    ) -> Vec<CheckError> {
        self.expr_types = expr_types;

        // Collect enum names, trait defs, impl table, and fn bounds.
        for item in &program.items {
            match item {
                Item::EnumDef(e) => {
                    self.known_enums.push(e.name.clone());
                    let variants = e.variants.iter().map(|v| v.name.clone()).collect();
                    self.enum_variants.insert(e.name.clone(), variants);
                }
                Item::TraitDef(t) => {
                    self.trait_defs.insert(t.name.clone(), t.clone());
                }
                Item::ImplBlock(blk) if !blk.trait_name.is_empty() => {
                    // Record: axon_type_name(for_type) implements trait_name.
                    self.impl_table
                        .entry(axon_type_name(&blk.for_type))
                        .or_default()
                        .insert(blk.trait_name.clone());
                }
                Item::FnDef(f) if !f.generic_bounds.is_empty() => {
                    self.fn_bounds.insert(f.name.clone(), f.generic_bounds.clone());
                }
                _ => {}
            }
        }

        // Validate impl blocks before checking bodies (E0501/E0502/E0503).
        for item in &program.items {
            if let Item::ImplBlock(blk) = item {
                self.check_impl_block(blk);
            }
        }

        for item in &program.items {
            self.check_item(item);
        }

        std::mem::take(&mut self.errors)
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Trait impl validation (E0501 / E0502 / E0503)
    // ─────────────────────────────────────────────────────────────────────────

    fn check_impl_block(&mut self, blk: &crate::ast::ImplBlock) {
        // E0501: Trait does not exist.
        let trait_def = match self.trait_defs.get(&blk.trait_name).cloned() {
            Some(t) => t,
            None => {
                self.errors.push(
                    CheckError::new(
                        E0501,
                        format!(
                            "trait `{}` not found — cannot implement unknown trait",
                            blk.trait_name
                        ),
                    )
                    .with_span(blk.span),
                );
                return; // E0502/E0503 are meaningless without a known trait
            }
        };

        let type_name = axon_type_name(&blk.for_type);
        let impl_method_names: std::collections::HashSet<&str> =
            blk.methods.iter().map(|m| m.name.as_str()).collect();

        // E0502: Required method(s) missing from impl block.
        for required in &trait_def.methods {
            if !impl_method_names.contains(required.name.as_str()) {
                self.errors.push(
                    CheckError::new(
                        E0502,
                        format!(
                            "impl of `{}` for `{type_name}` is missing method `{}`",
                            blk.trait_name, required.name
                        ),
                    )
                    .with_span(blk.span),
                );
            }
        }

        // E0503: Method signature mismatch.
        for impl_method in &blk.methods {
            let trait_method = match trait_def
                .methods
                .iter()
                .find(|m| m.name == impl_method.name)
            {
                Some(m) => m,
                None => continue, // extra method not in trait — not an error at this level
            };

            // Compare parameter count (excluding `self` if present).
            let trait_arity = trait_method.params.len();
            let impl_arity = impl_method.params.len();
            if trait_arity != impl_arity {
                self.errors.push(
                    CheckError::new(
                        E0503,
                        format!(
                            "method `{}` in impl of `{}` for `{type_name}` has {} parameter{}, \
                             but trait declares {}",
                            impl_method.name,
                            blk.trait_name,
                            impl_arity,
                            if impl_arity == 1 { "" } else { "s" },
                            trait_arity,
                        ),
                    )
                    .with_span(impl_method.span),
                );
                continue;
            }

            // Compare parameter types.
            for (i, (impl_param, trait_param)) in impl_method
                .params
                .iter()
                .zip(trait_method.params.iter())
                .enumerate()
            {
                if !axon_types_compatible(&impl_param.ty, &trait_param.ty) {
                    self.errors.push(
                        CheckError::new(
                            E0503,
                            format!(
                                "method `{}` in impl of `{}` for `{type_name}`: \
                                 parameter {} (`{}`) has type `{}`, but trait expects `{}`",
                                impl_method.name,
                                blk.trait_name,
                                i,
                                impl_param.name,
                                axon_type_display(&impl_param.ty),
                                axon_type_display(&trait_param.ty),
                            ),
                        )
                        .with_span(impl_method.span),
                    );
                }
            }

            // Compare return types.
            let impl_ret = impl_method.return_type.as_ref();
            let trait_ret = trait_method.return_type.as_ref();
            match (impl_ret, trait_ret) {
                (Some(a), Some(b)) if !axon_types_compatible(a, b) => {
                    self.errors.push(
                        CheckError::new(
                            E0503,
                            format!(
                                "method `{}` in impl of `{}` for `{type_name}`: \
                                 return type is `{}`, but trait declares `{}`",
                                impl_method.name,
                                blk.trait_name,
                                axon_type_display(a),
                                axon_type_display(b),
                            ),
                        )
                        .with_span(impl_method.span),
                    );
                }
                (None, Some(b)) => {
                    self.errors.push(
                        CheckError::new(
                            E0503,
                            format!(
                                "method `{}` in impl of `{}` for `{type_name}`: \
                                 missing return type (trait declares `{}`)",
                                impl_method.name,
                                blk.trait_name,
                                axon_type_display(b),
                            ),
                        )
                        .with_span(impl_method.span),
                    );
                }
                (Some(a), None) => {
                    self.errors.push(
                        CheckError::new(
                            E0503,
                            format!(
                                "method `{}` in impl of `{}` for `{type_name}`: \
                                 has return type `{}` but trait declares no return",
                                impl_method.name,
                                blk.trait_name,
                                axon_type_display(a),
                            ),
                        )
                        .with_span(impl_method.span),
                    );
                }
                _ => {}
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Item-level
    // ─────────────────────────────────────────────────────────────────────────

    fn check_item(&mut self, item: &Item) {
        match item {
            Item::FnDef(f) => self.check_fn(f),
            Item::TypeDef(td) => {
                // R08: validate field type annotations.
                // Generic params of the typedef (e.g. A, B in `type Pair<A,B>`) are valid.
                let prev_generics = std::mem::replace(
                    &mut self.current_generic_params,
                    td.generic_params.iter().cloned().collect(),
                );
                for field in &td.fields {
                    let path = format!("#typedef_{}.field_{}", td.name, field.name);
                    self.check_axon_type(&field.ty, &path);
                }
                self.current_generic_params = prev_generics;
            }
            Item::ImplBlock(blk) => {
                for m in &blk.methods {
                    self.check_fn(m);
                }
            }
            Item::EnumDef(_) | Item::ModDecl(_) | Item::UseDecl(_) | Item::TraitDef(_)
            | Item::LetDef { .. } => {}
        }
    }

    fn check_fn(&mut self, f: &FnDef) {
        // Bring generic type params into scope so R08 doesn't flag them as unknown.
        let prev_generics = std::mem::replace(
            &mut self.current_generic_params,
            f.generic_params.iter().cloned().collect(),
        );
        // Seed `current_span` from the function header so any diagnostic raised
        // before we descend into the body is at least pointed at the function.
        let prev_span = self.current_span;
        if !f.span.is_dummy() {
            self.current_span = f.span;
        }

        // R08: validate parameter type annotations.
        for param in &f.params {
            let path = format!("#fn_{}.param_{}", f.name, param.name);
            self.check_axon_type(&param.ty, &path);
        }

        // R08: validate return type annotation.
        if let Option::Some(ret_ty) = &f.return_type {
            let path = format!("#fn_{}.return_type", f.name);
            self.check_axon_type(&ret_ty.clone(), &path);
        }

        // Resolve the declared return type for R03 / R07 checks.
        let resolved_ret = f
            .return_type
            .as_ref()
            .map(axon_type_to_type)
            .unwrap_or(Type::Unit);

        let prev_ret = self.current_ret_ty.replace(resolved_ret);

        let fn_path = format!("#fn_{}", f.name);
        let mut scope: HashMap<String, Type> = HashMap::new();

        // Seed scope with parameters.
        for param in &f.params {
            scope.insert(param.name.clone(), axon_type_to_type(&param.ty));
        }

        self.check_expr(&f.body, &format!("{fn_path}.body"), &mut scope);

        self.current_ret_ty = prev_ret;
        self.current_generic_params = prev_generics;
        self.current_span = prev_span;
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Statement
    // ─────────────────────────────────────────────────────────────────────────

    fn check_stmt(
        &mut self,
        stmt: &Stmt,
        node_path: &str,
        scope: &mut HashMap<String, Type>,
    ) {
        // Track the statement span so deeply-nested errors can attach a
        // useful source location (rustc-style `file:line:col`).
        if !stmt.span.is_dummy() {
            self.current_span = stmt.span;
        }
        self.check_expr(&stmt.expr, node_path, scope);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Expression
    // ─────────────────────────────────────────────────────────────────────────

    fn check_expr(
        &mut self,
        expr: &Expr,
        node_path: &str,
        scope: &mut HashMap<String, Type>,
    ) {
        match expr {
            // ── Block ────────────────────────────────────────────────────────
            Expr::Block(stmts) => {
                let n = stmts.len();
                for (i, stmt) in stmts.iter().enumerate() {
                    let stmt_path = format!("{node_path}.stmt_{i}");
                    let is_last = i + 1 == n;

                    if is_last {
                        self.check_stmt(stmt, &stmt_path, scope);
                        // R07: the final expression of the function body must
                        // match the declared return type.
                        if node_path.ends_with(".body") {
                            let expr_ty =
                                self.resolve_expr_type(&stmt.expr, &stmt_path, scope);
                            self.check_return_type_match(&expr_ty, node_path, &stmt_path);
                        }
                    } else {
                        // Non-final statement: R02 if a call returns Result
                        // and the result is not being stored or propagated.
                        self.check_stmt_result_ignored(&stmt.expr, &stmt_path, scope);
                        self.check_stmt(stmt, &stmt_path, scope);
                    }
                }
            }

            // ── Binding forms ────────────────────────────────────────────────
            // The RHS is "used" (stored), so R02 does not apply.
            Expr::Let { name, value }
            | Expr::Own { name, value }
            | Expr::RefBind { name, value } => {
                let val_path = format!("{node_path}.value");
                self.check_expr(value, &val_path, scope);
                let ty = self.resolve_expr_type(value, &val_path, scope);
                scope.insert(name.clone(), ty);
            }

            // ── Call ─────────────────────────────────────────────────────────
            Expr::Call { callee, args } => {
                self.check_expr(callee, &format!("{node_path}.callee"), scope);
                for (i, arg) in args.iter().enumerate() {
                    self.check_expr(arg, &format!("{node_path}.arg_{i}"), scope);
                }
                // R05 / R06 — only for named (direct) calls.
                if let Expr::Ident(name) = callee.as_ref() {
                    // Fix #3: detect calling a local variable that is not a function.
                    if let Some(ty) = scope.get(name.as_str()) {
                        if !matches!(ty, Type::Fn(_, _) | Type::Unknown | Type::Deferred(_) | Type::Var(_)) {
                            let file = self.file.clone();
                            self.errors.push(
                                CheckError::new(
                                    E0306,
                                    format!("cannot call non-function value '{name}'"),
                                )
                                .node(node_path)
                                .at(&file, 0, 0)
                                .fix(format!("'{name}' is a local variable of type {}, not a function", ty.display())),
                            );
                        }
                    }
                    self.check_call_arity_and_types(name, args, node_path, scope);
                }
            }

            // ── MethodCall ───────────────────────────────────────────────────
            Expr::MethodCall { receiver, method: _, args } => {
                self.check_expr(receiver, &format!("{node_path}.receiver"), scope);
                for (i, arg) in args.iter().enumerate() {
                    self.check_expr(arg, &format!("{node_path}.arg_{i}"), scope);
                }
            }

            // ── BinOp ────────────────────────────────────────────────────────
            Expr::BinOp { op, left, right } => {
                let lpath = format!("{node_path}.left");
                let rpath = format!("{node_path}.right");
                self.check_expr(left, &lpath, scope);
                self.check_expr(right, &rpath, scope);
                // R01: arithmetic operands must not be bare Option<T>.
                let lty = self.resolve_expr_type(left, &lpath, scope);
                let rty = self.resolve_expr_type(right, &rpath, scope);
                self.check_not_option_used_as_value(&lty, &lpath);
                self.check_not_option_used_as_value(&rty, &rpath);
                // Fix #4: arithmetic operands must be numeric types.
                use crate::ast::BinOp;
                if matches!(op, BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem) {
                    self.check_numeric_operand(&lty, &lpath);
                    self.check_numeric_operand(&rty, &rpath);
                }
            }

            // ── UnaryOp ──────────────────────────────────────────────────────
            Expr::UnaryOp { op, operand } => {
                use crate::ast::UnaryOp;
                let opath = format!("{node_path}.operand");
                self.check_expr(operand, &opath, scope);
                let ty = self.resolve_expr_type(operand, &opath, scope);
                self.check_not_option_used_as_value(&ty, &opath);
                // Fix #14: unary negation requires a numeric operand.
                if matches!(op, UnaryOp::Neg) {
                    self.check_numeric_operand(&ty, &opath);
                }
                // `Not` already constrained to Bool via inference.
                // `Ref` is transparent in Phase 1.
            }

            // ── Question (?) ─────────────────────────────────────────────────
            Expr::Question(inner) => {
                // R03: `?` is only valid inside a Result-returning function.
                let file = self.file.clone();
                let span = self.current_span;
                match &self.current_ret_ty {
                    Option::Some(ret) if ret.is_result() => {}
                    Option::Some(ret) => {
                        let ret_display = ret.display();
                        self.errors.push(
                            CheckError::new(
                                E0303,
                                format!(
                                    "the `?` operator can only be used in a function that returns `Result`, \
                                     but the enclosing function returns `{ret_display}`"
                                ),
                            )
                            .node(node_path)
                            .at(&file, 0, 0)
                            .with_span(span)
                            .expected("Result<T, E>")
                            .found(ret_display)
                            .fix("change the function's return type to `Result<T, E>`, or handle the error with `match`"),
                        );
                    }
                    Option::None => {
                        self.errors.push(
                            CheckError::new(
                                E0303,
                                "the `?` operator was used outside of a function",
                            )
                                .node(node_path)
                                .at(&file, 0, 0)
                                .with_span(span)
                                .fix("only use `?` inside a function that returns `Result<T, E>`"),
                        );
                    }
                }
                self.check_expr(inner, &format!("{node_path}.inner"), scope);
            }

            // ── Match ────────────────────────────────────────────────────────
            Expr::Match { subject, arms } => {
                let subj_path = format!("{node_path}.subject");
                self.check_expr(subject, &subj_path, scope);

                // R04: exhaustiveness for Option / Result.
                let subj_ty = self.resolve_expr_type(subject, &subj_path, scope);
                self.check_match_exhaustiveness(&subj_ty, arms, node_path);

                for (i, arm) in arms.iter().enumerate() {
                    if let Option::Some(guard) = &arm.guard {
                        self.check_expr(
                            guard,
                            &format!("{node_path}.arm_{i}.guard"),
                            scope,
                        );
                    }
                    self.check_expr(&arm.body, &format!("{node_path}.arm_{i}.body"), scope);
                }
            }

            // ── If ───────────────────────────────────────────────────────────
            Expr::If { cond, then, else_ } => {
                self.check_expr(cond, &format!("{node_path}.cond"), scope);
                self.check_expr(then, &format!("{node_path}.then"), scope);
                if let Option::Some(e) = else_ {
                    self.check_expr(e, &format!("{node_path}.else"), scope);
                }
            }

            // ── Return ───────────────────────────────────────────────────────
            Expr::Return(val) => {
                if let Option::Some(v) = val {
                    let vpath = format!("{node_path}.value");
                    self.check_expr(v, &vpath, scope);
                    // R07: returned value must match declared return type.
                    let val_ty = self.resolve_expr_type(v, &vpath, scope);
                    self.check_return_type_match(&val_ty, node_path, &vpath);
                } else {
                    // Bare `return;` implies Unit.
                    self.check_return_type_match(&Type::Unit, node_path, node_path);
                }
            }

            // ── FieldAccess ──────────────────────────────────────────────────
            Expr::FieldAccess { receiver, field } => {
                let recv_path = format!("{node_path}.receiver");
                self.check_expr(receiver, &recv_path, scope);
                // R11
                let recv_ty = self.resolve_expr_type(receiver, &recv_path, scope);
                self.check_field_access(&recv_ty, field, node_path);
            }

            // ── Index ────────────────────────────────────────────────────────
            Expr::Index { receiver, index } => {
                self.check_expr(receiver, &format!("{node_path}.receiver"), scope);
                self.check_expr(index, &format!("{node_path}.index"), scope);
            }

            // ── Spawn / Comptime ─────────────────────────────────────────────
            Expr::Spawn(inner) | Expr::Comptime(inner) => {
                self.check_expr(inner, &format!("{node_path}.inner"), scope);
            }

            // ── Select ───────────────────────────────────────────────────────
            Expr::Select(arms) => {
                for (i, arm) in arms.iter().enumerate() {
                    self.check_expr(&arm.recv, &format!("{node_path}.arm_{i}.recv"), scope);
                    self.check_expr(&arm.body, &format!("{node_path}.arm_{i}.body"), scope);
                }
            }

            // ── Lambda ───────────────────────────────────────────────────────
            // Introduce a fresh return-type context so `?` / return checks
            // inside the lambda do not bleed into the outer function.
            Expr::Lambda { params: _, body, captures: _ } => {
                let prev = self.current_ret_ty.take();
                self.check_expr(body, &format!("{node_path}.body"), scope);
                self.current_ret_ty = prev;
            }

            // ── Wrapper constructors ─────────────────────────────────────────
            Expr::Ok(inner) | Expr::Err(inner) | Expr::Some(inner) => {
                self.check_expr(inner, &format!("{node_path}.inner"), scope);
            }

            // ── While ────────────────────────────────────────────────────────
            Expr::While { cond, body } => {
                self.check_expr(cond, &format!("{node_path}.cond"), scope);
                for (i, stmt) in body.iter().enumerate() {
                    self.check_stmt(stmt, &format!("{node_path}.body_stmt_{i}"), scope);
                }
            }
            Expr::WhileLet { expr, body, .. } => {
                self.check_expr(expr, &format!("{node_path}.while_let_expr"), scope);
                for (i, stmt) in body.iter().enumerate() {
                    self.check_stmt(stmt, &format!("{node_path}.while_let_body_{i}"), scope);
                }
            }
            Expr::For { var, start, end, body, .. } => {
                self.check_expr(start, &format!("{node_path}.start"), scope);
                self.check_expr(end, &format!("{node_path}.end"), scope);
                let mut inner = scope.clone();
                inner.insert(var.clone(), crate::types::Type::I64);
                for (i, stmt) in body.iter().enumerate() {
                    self.check_stmt(stmt, &format!("{node_path}.for_body_{i}"), &mut inner);
                }
            }

            // ── Assign (rebind existing local) ────────────────────────────────
            Expr::Assign { name, value } => {
                let val_path = format!("{node_path}.value");
                self.check_expr(value, &val_path, scope);
                let ty = self.resolve_expr_type(value, &val_path, scope);
                scope.insert(name.clone(), ty);
            }

            // ── FmtStr: check each interpolated sub-expression ──────────────
            Expr::FmtStr { parts } => {
                for part in parts {
                    if let FmtPart::Expr(e) = part {
                        self.check_expr(e, &format!("{node_path}.fmt_part"), scope);
                    }
                }
            }

            // ── Leaves ───────────────────────────────────────────────────────
            Expr::Ident(_)
            | Expr::Literal(_)
            | Expr::None
            | Expr::Array(_)
            | Expr::StructLit { .. }
            | Expr::Break
            | Expr::Continue => {}
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // R02 — detect ignored Result at statement level
    // ─────────────────────────────────────────────────────────────────────────

    /// Only non-final statement expressions are checked here.
    /// Binding forms, `?`, and `match` all "consume" the value.
    fn check_stmt_result_ignored(
        &mut self,
        expr: &Expr,
        node_path: &str,
        scope: &mut HashMap<String, Type>,
    ) {
        match expr {
            // These consume / store the Result — not ignored.
            Expr::Let { .. } | Expr::Own { .. } | Expr::RefBind { .. } => {}
            Expr::Question(_) => {}
            Expr::Match { .. } => {}
            // Any call-like expression at statement level whose type is Result.
            Expr::Call { .. } | Expr::MethodCall { .. } => {
                let ty = self.resolve_expr_type(expr, node_path, scope);
                if ty.is_result() && !ty.is_deferred() {
                    let file = self.file.clone();
                    let span = self.current_span;
                    let ty_disp = ty.display();
                    self.errors.push(
                        CheckError::new(
                            E0302,
                            format!(
                                "the `{ty_disp}` returned by this call must be used — \
                                 unhandled errors are silently dropped",
                            ),
                        )
                        .node(node_path)
                        .at(&file, 0, 0)
                        .with_span(span)
                        .found(ty_disp)
                        .fix("add `?` to propagate the error, or wrap the call in \
                              `match call() { Ok(v) => v, Err(e) => /* handle */ }`"),
                    );
                }
            }
            _ => {}
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // R01 — Option<T> used directly as a value
    // ─────────────────────────────────────────────────────────────────────────

    fn check_not_option_used_as_value(&mut self, ty: &Type, node_path: &str) {
        if ty.is_deferred() {
            return; // R12: deferred types are transparent
        }
        if ty.is_option() {
            let file = self.file.clone();
            let span = self.current_span;
            let inner = match ty {
                Type::Option(inner) => inner.display(),
                _ => "T".to_string(),
            };
            self.errors.push(
                CheckError::new(
                    E0301,
                    format!(
                        "value of type `Option<{inner}>` cannot be used directly — \
                         the `Some`/`None` cases must be handled first",
                    ),
                )
                    .node(node_path)
                    .at(&file, 0, 0)
                    .with_span(span)
                    .expected(inner.clone())
                    .found(format!("Option<{inner}>"))
                    .fix(format!(
                        "use `x.unwrap_or(default)` or `match x {{ Some(v) => v, None => default }}` \
                         to obtain a `{inner}`"
                    )),
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Fix #4 — arithmetic operands must be numeric
    // ─────────────────────────────────────────────────────────────────────────

    fn check_numeric_operand(&mut self, ty: &Type, node_path: &str) {
        // Skip unknowns, vars, deferred — only fire on concrete non-numeric types.
        if ty.is_deferred() || matches!(ty, Type::Unknown | Type::Var(_)) {
            return;
        }
        if !ty.is_numeric() {
            let file = self.file.clone();
            self.errors.push(
                CheckError::new(
                    E0102,
                    format!(
                        "arithmetic operand has non-numeric type {}",
                        ty.display()
                    ),
                )
                .node(node_path)
                .at(&file, 0, 0)
                .expected("numeric type (i64, f64, i32, …)")
                .found(ty.display()),
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // R05 / R06 — argument count and types
    // ─────────────────────────────────────────────────────────────────────────

    fn check_call_arity_and_types(
        &mut self,
        name: &str,
        args: &[Expr],
        node_path: &str,
        scope: &mut HashMap<String, Type>,
    ) {
        let sig = match self.fn_sigs.get(name).cloned() {
            Option::Some(s) => s,
            Option::None => return, // Unknown function — inference handles it.
        };

        // R05 — argument count.
        if args.len() != sig.params.len() {
            let file = self.file.clone();
            let span = self.current_span;
            let expected_n = sig.params.len();
            let got_n = args.len();
            // Spell out the expected signature so the user sees what's missing.
            let sig_render = if expected_n == 0 {
                format!("`{name}()`")
            } else {
                let params: Vec<String> =
                    sig.params.iter().map(|p| p.display()).collect();
                format!("`{name}({})`", params.join(", "))
            };
            let hint = if got_n < expected_n {
                let missing = expected_n - got_n;
                format!("you supplied {got_n}; supply {missing} more (signature: {sig_render})")
            } else {
                let extra = got_n - expected_n;
                format!("you supplied {got_n}; remove {extra} (signature: {sig_render})")
            };
            self.errors.push(
                CheckError::new(
                    E0305,
                    format!(
                        "function `{name}` takes {expected_n} argument{} but {got_n} {} supplied",
                        if expected_n == 1 { "" } else { "s" },
                        if got_n == 1 { "was" } else { "were" },
                    ),
                )
                .node(node_path)
                .at(&file, 0, 0)
                .with_span(span)
                .expected(expected_n.to_string())
                .found(got_n.to_string())
                .fix(hint),
            );
            // Continue so R06 can fire on the arguments we do have.
        }

        // R06 — argument types.
        for (i, (arg, param_ty)) in args.iter().zip(sig.params.iter()).enumerate() {
            let arg_path = format!("{node_path}.arg_{i}");
            let arg_ty = self.resolve_expr_type(arg, &arg_path, scope);

            // Skip when either side is unresolved or deferred (R12).
            // Also skip when either type recursively contains TypeParam/Unknown (generic callers).
            if arg_ty == Type::Unknown
                || *param_ty == Type::Unknown
                || matches!(arg_ty, Type::Var(_))
                || matches!(param_ty, Type::Var(_))
                || arg_ty.is_deferred()
                || param_ty.is_deferred()
                || type_contains_unresolved(&arg_ty)
                || type_contains_unresolved(param_ty)
            {
                continue;
            }

            // R01 specialisation: if the arg is Option<T> and the param
            // expects the inner T, emit E0301 rather than E0306.
            if let Type::Option(inner) = &arg_ty {
                if **inner == *param_ty {
                    let file = self.file.clone();
                    let span = self.current_span;
                    self.errors.push(
                        CheckError::new(
                            E0301,
                            format!(
                                "argument {i} of `{name}` has type `Option<{inner_disp}>`, but \
                                 the parameter expects `{param_disp}` — the `Option` must be \
                                 unwrapped first",
                                inner_disp = inner.display(),
                                param_disp = param_ty.display(),
                            ),
                        )
                        .node(&arg_path)
                        .at(&file, 0, 0)
                        .with_span(span)
                        .expected(param_ty.display())
                        .found(arg_ty.display())
                        .fix(format!(
                            "use `arg.unwrap_or(default)` or `match arg {{ Some(v) => v, None => default }}` \
                             to obtain a `{}`",
                            param_ty.display()
                        )),
                    );
                    continue;
                }
            }

            // Generic R06 type mismatch.
            // Fix #16: include function name and parameter index for clarity.
            // Allow implicit integer widening (e.g. i32 arg where i64 expected).
            // Allow concrete type coercion to dyn Trait when the type implements the trait.
            let dyn_coercion_ok = if let Type::DynTrait(trait_name) = &*param_ty {
                let concrete_name = match &arg_ty {
                    Type::Struct(n) | Type::Enum(n) => Some(n.as_str()),
                    _ => None,
                };
                concrete_name.map(|n| {
                    self.impl_table
                        .get(n)
                        .map(|set| set.contains(trait_name.as_str()))
                        .unwrap_or(false)
                }).unwrap_or(false)
            } else {
                false
            };
            if arg_ty != *param_ty && !is_integer_widening(&arg_ty, param_ty) && !dyn_coercion_ok {
                let file = self.file.clone();
                let span = self.current_span;
                let expected_disp = param_ty.display();
                let found_disp = arg_ty.display();
                // Be specific when the mismatch is a common conversion the user
                // can fix in-place (e.g. integer narrowing, str ↔ String, etc.).
                let hint = if is_integer_widening(param_ty, &arg_ty) {
                    format!(
                        "argument is `{found_disp}` but parameter is `{expected_disp}`; \
                         narrow with `as {expected_disp}` (truncation may occur)"
                    )
                } else if matches!(arg_ty, Type::I64 | Type::I32 | Type::I16 | Type::I8)
                    && matches!(param_ty, Type::F64 | Type::F32)
                {
                    format!("convert with `as {expected_disp}` to widen the integer to a float")
                } else {
                    format!(
                        "expected `{expected_disp}`, found `{found_disp}` — \
                         change the argument's type or cast with `as {expected_disp}` if compatible"
                    )
                };
                self.errors.push(
                    CheckError::new(
                        E0306,
                        format!(
                            "argument {i} of `{name}` has the wrong type: \
                             expected `{expected_disp}`, found `{found_disp}`",
                        ),
                    )
                    .node(&arg_path)
                    .at(&file, 0, 0)
                    .with_span(span)
                    .expected(expected_disp.clone())
                    .found(found_disp.clone())
                    .fix(hint),
                );
            }
        }

        // E0504 — trait bound satisfaction check.
        // For each (type_param, bounds) on this function, find which args use that
        // type param and check that the concrete resolved type implements each bound.
        self.check_trait_bounds(name, args, node_path, scope, &sig);
    }

    fn check_trait_bounds(
        &mut self,
        fn_name: &str,
        args: &[Expr],
        node_path: &str,
        scope: &mut HashMap<String, Type>,
        sig: &FnSig,
    ) {
        let bounds = match self.fn_bounds.get(fn_name).cloned() {
            Some(b) if !b.is_empty() => b,
            _ => return,
        };

        // For each (param_name → concrete Type) mapping derived from inference,
        // we use the sig's param types: if sig param is TypeParam("T"), match against
        // the concrete arg type resolved at the call site.
        for (type_param_name, trait_names) in &bounds {
            // Find which parameter positions declare this type param.
            for (i, param_ty) in sig.params.iter().enumerate() {
                if !matches!(param_ty, Type::TypeParam(n) if n == type_param_name) {
                    continue;
                }
                let Some(arg) = args.get(i) else { continue };
                let arg_path = format!("{node_path}.arg_{i}");
                let arg_ty = self.resolve_expr_type(arg, &arg_path, scope);

                // Skip if unresolved.
                if matches!(arg_ty, Type::Unknown | Type::Var(_) | Type::TypeParam(_)) {
                    continue;
                }

                // Get the type name to look up in impl_table.
                let type_name = match &arg_ty {
                    Type::Struct(n) | Type::Enum(n) => n.clone(),
                    Type::I64 => "i64".into(),
                    Type::I32 => "i32".into(),
                    Type::I16 => "i16".into(),
                    Type::I8  => "i8".into(),
                    Type::U64 => "u64".into(),
                    Type::U32 => "u32".into(),
                    Type::U16 => "u16".into(),
                    Type::U8  => "u8".into(),
                    Type::F64 => "f64".into(),
                    Type::F32 => "f32".into(),
                    Type::Bool => "bool".into(),
                    Type::Str  => "str".into(),
                    _ => continue,
                };

                for trait_name in trait_names {
                    let implements = self
                        .impl_table
                        .get(&type_name)
                        .map(|set| set.contains(trait_name.as_str()))
                        .unwrap_or(false);

                    if !implements {
                        let file = self.file.clone();
                        self.errors.push(
                            CheckError::new(
                                E0504,
                                format!(
                                    "fn `{fn_name}` requires `{type_param_name}: {trait_name}`, \
                                     but `{type_name}` does not implement `{trait_name}`",
                                ),
                            )
                            .node(&arg_path)
                            .at(&file, 0, 0)
                            .fix(format!("add `impl {trait_name} for {type_name} {{ ... }}`")),
                        );
                    }
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // R04 — match exhaustiveness for Option and Result
    // ─────────────────────────────────────────────────────────────────────────

    fn check_match_exhaustiveness(
        &mut self,
        subject_ty: &Type,
        arms: &[MatchArm],
        node_path: &str,
    ) {
        if subject_ty.is_deferred() {
            return;
        }

        // A wildcard pattern or a plain identifier covers all constructors.
        let has_wildcard = arms
            .iter()
            .any(|arm| matches!(arm.pattern, Pattern::Wildcard | Pattern::Ident(_)));

        if has_wildcard {
            return;
        }

        let file = self.file.clone();
        match subject_ty {
            Type::Option(_) => {
                let has_some = arms.iter().any(|a| matches!(a.pattern, Pattern::Some(_)));
                let has_none = arms.iter().any(|a| matches!(a.pattern, Pattern::None));
                if !has_some {
                    self.errors.push(
                        CheckError::new(E0304, "non-exhaustive match — missing Some(_) arm")
                            .node(node_path)
                            .at(&file, 0, 0)
                            .fix("add arm: Some(v) => { /* handle value */ }"),
                    );
                }
                if !has_none {
                    self.errors.push(
                        CheckError::new(E0304, "non-exhaustive match — missing None arm")
                            .node(node_path)
                            .at(&file, 0, 0)
                            .fix("add arm: None => { /* handle missing */ }"),
                    );
                }
            }
            Type::Result(_, _) => {
                let has_ok = arms.iter().any(|a| matches!(a.pattern, Pattern::Ok(_)));
                let has_err = arms.iter().any(|a| matches!(a.pattern, Pattern::Err(_)));
                if !has_ok {
                    self.errors.push(
                        CheckError::new(E0304, "non-exhaustive match — missing Ok(_) arm")
                            .node(node_path)
                            .at(&file, 0, 0)
                            .fix("add arm: Ok(v) => { /* handle success */ }"),
                    );
                }
                if !has_err {
                    self.errors.push(
                        CheckError::new(E0304, "non-exhaustive match — missing Err(_) arm")
                            .node(node_path)
                            .at(&file, 0, 0)
                            .fix("add arm: Err(e) => { /* handle error */ }"),
                    );
                }
            }
            // Fix #10: exhaustiveness for user-defined enums.
            Type::Enum(enum_name) => {
                // Look up the variant list for this enum.
                let variants = self.enum_variants.get(enum_name.as_str()).cloned();
                if let Some(variants) = variants {
                    // Collect which variant names appear in StructLit patterns.
                    // Enum variant patterns appear as Pattern::Struct { name: "EnumName::VariantName", .. }
                    // or as a plain Pattern::Ident if the user writes the variant name as-is.
                    let covered: std::collections::HashSet<String> = arms
                        .iter()
                        .filter_map(|arm| match &arm.pattern {
                            Pattern::Struct { name, .. } => {
                                // "EnumName::VariantName" → extract variant name
                                if let Some((_, variant)) = name.split_once("::") {
                                    Some(variant.to_string())
                                } else {
                                    Some(name.clone())
                                }
                            }
                            Pattern::Ident(name) => Some(name.clone()),
                            _ => None,
                        })
                        .collect();

                    for variant in &variants {
                        if !covered.contains(variant) {
                            self.errors.push(
                                CheckError::new(
                                    E0304,
                                    format!(
                                        "non-exhaustive match on enum '{enum_name}' — \
                                         missing variant '{variant}'"
                                    ),
                                )
                                .node(node_path)
                                .at(&file, 0, 0)
                                .fix(format!(
                                    "add arm: {enum_name}::{variant} {{ .. }} => {{ /* handle */ }}"
                                )),
                            );
                        }
                    }
                }
            }
            _ => {}
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // R07 — return type agreement
    // ─────────────────────────────────────────────────────────────────────────

    fn check_return_type_match(
        &mut self,
        val_ty: &Type,
        node_path: &str,
        _val_path: &str,
    ) {
        // R12: deferred types are always compatible.
        if val_ty.is_deferred() {
            return;
        }

        let ret_ty = match &self.current_ret_ty {
            Option::Some(t) => t.clone(),
            Option::None => return,
        };

        if ret_ty.is_deferred() {
            return;
        }

        // Unknown on either side (including nested Unknown): let inference report the error.
        if *val_ty == Type::Unknown
            || ret_ty == Type::Unknown
            || matches!(val_ty, Type::Var(_))
            || matches!(&ret_ty, Type::Var(_))
            || type_contains_unresolved(val_ty)
            || type_contains_unresolved(&ret_ty)
        {
            return;
        }

        if *val_ty != ret_ty {
            let file = self.file.clone();
            let span = self.current_span;
            let expected = ret_ty.display();
            let found = val_ty.display();
            // Tailor the suggestion to common shapes: returning a value where
            // `()`/`Unit` is expected, or a bare T where Result<T,_> is expected.
            let hint = match (&ret_ty, val_ty) {
                (Type::Result(ok, _), v) if &**ok == v => {
                    format!("wrap the value with `Ok(...)` to return `{expected}`")
                }
                (Type::Option(inner), v) if &**inner == v => {
                    format!("wrap the value with `Some(...)` to return `{expected}`")
                }
                (Type::Unit, _) => {
                    "the function returns `()`; remove the trailing expression \
                     or end the block with `;`".to_string()
                }
                _ => format!(
                    "the function declares `-> {expected}`, but the body produces `{found}` — \
                     adjust the final expression (or change the declared return type)"
                ),
            };
            self.errors.push(
                CheckError::new(
                    E0307,
                    format!(
                        "return type mismatch: expected `{expected}`, found `{found}`",
                    ),
                )
                .node(node_path)
                .at(&file, 0, 0)
                .with_span(span)
                .expected(expected)
                .found(found)
                .fix(hint),
            );
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // R08 — unknown type annotation
    // ─────────────────────────────────────────────────────────────────────────

    fn check_axon_type(&mut self, ty: &AxonType, node_path: &str) {
        match ty {
            AxonType::Named(name) => {
                // Suppress E0308 for names that are generic type parameters of the
                // enclosing function (e.g. `T` in `fn identity<T>(x: T) -> T`).
                if self.current_generic_params.contains(name.as_str()) {
                    return;
                }
                let known_enums = self.known_enums.clone();
                if !is_known_type_name(name, &self.struct_fields, &known_enums) {
                    let mut candidates: Vec<String> =
                        PRIMITIVE_NAMES.iter().map(|s| s.to_string()).collect();
                    for k in self.struct_fields.keys() {
                        candidates.push(k.clone());
                    }
                    for k in &known_enums {
                        candidates.push(k.clone());
                    }
                    let fix = match closest_name(name, &candidates) {
                        Option::Some(s) => format!("did you mean '{s}'?"),
                        Option::None => "check the type name".to_string(),
                    };
                    let file = self.file.clone();
                    self.errors.push(
                        CheckError::new(E0308, format!("unknown type '{name}'"))
                            .node(node_path)
                            .at(&file, 0, 0)
                            .fix(fix),
                    );
                }
            }
            AxonType::Result { ok, err } => {
                self.check_axon_type(ok, &format!("{node_path}.ok"));
                self.check_axon_type(err, &format!("{node_path}.err"));
            }
            AxonType::Option(inner) => {
                self.check_axon_type(inner, &format!("{node_path}.inner"));
            }
            AxonType::Chan(inner) | AxonType::Slice(inner) | AxonType::Ref(inner) => {
                self.check_axon_type(inner, &format!("{node_path}.inner"));
            }
            AxonType::Generic { base, args } => {
                // Validate the base name (deferred prefixes are always OK).
                self.check_axon_type(
                    &AxonType::Named(base.clone()),
                    &format!("{node_path}.base"),
                );
                for (i, arg) in args.iter().enumerate() {
                    self.check_axon_type(arg, &format!("{node_path}.arg_{i}"));
                }
            }
            AxonType::Fn { params, ret } => {
                for (i, p) in params.iter().enumerate() {
                    self.check_axon_type(p, &format!("{node_path}.param_{i}"));
                }
                self.check_axon_type(ret, &format!("{node_path}.ret"));
            }
            AxonType::TypeParam(_) | AxonType::DynTrait(_) => {}
            AxonType::Tuple(elems) => {
                for (i, elem) in elems.iter().enumerate() {
                    self.check_axon_type(elem, &format!("{node_path}.elem_{i}"));
                }
            }
            AxonType::Union(members) => {
                // Each branch of a union is independently checked; an unknown
                // branch still triggers E0308 against that branch alone.
                for (i, m) in members.iter().enumerate() {
                    self.check_axon_type(m, &format!("{node_path}.union_{i}"));
                }
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // R11 — field access on non-struct
    // ─────────────────────────────────────────────────────────────────────────

    fn check_field_access(&mut self, recv_ty: &Type, field: &str, node_path: &str) {
        if recv_ty.is_deferred() {
            return; // R12
        }

        match recv_ty {
            Type::Struct(struct_name) => {
                match self.struct_fields.get(struct_name).cloned() {
                    Option::Some(fields) => {
                        if !fields.iter().any(|(n, _)| n == field) {
                            let field_names: Vec<String> = fields.iter().map(|(n, _)| n.clone()).collect();
                            let file = self.file.clone();
                            self.errors.push(
                                CheckError::new(
                                    E0401,
                                    format!("struct '{}' has no field '{field}'", struct_name),
                                )
                                .node(node_path)
                                .at(&file, 0, 0)
                                .found(field)
                                .fix(format!("'{struct_name}' fields: {}", field_names.join(", "))),
                            );
                        }
                    }
                    Option::None => {
                        let file = self.file.clone();
                        self.errors.push(
                            CheckError::new(
                                E0401,
                                format!(
                                    "unknown struct '{}' — cannot access field '{field}'",
                                    struct_name
                                ),
                            )
                            .node(node_path)
                            .at(&file, 0, 0),
                        );
                    }
                }
            }
            Type::Unknown => {
                // Let inference report the error.
            }
            other => {
                let file = self.file.clone();
                self.errors.push(
                    CheckError::new(
                        E0401,
                        format!("{} has no field '{field}'", other.display()),
                    )
                    .node(node_path)
                    .at(&file, 0, 0)
                    .found(field),
                );
            }
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Type resolution
    // ─────────────────────────────────────────────────────────────────────────

    /// Return the resolved type for an expression.
    ///
    /// Primary source: `expr_types` map (populated by inference).
    /// Fallback: lightweight syntactic analysis of the expression itself.
    fn resolve_expr_type(
        &self,
        expr: &Expr,
        node_path: &str,
        scope: &HashMap<String, Type>,
    ) -> Type {
        // 1. Use inference-provided type when available.
        if let Option::Some(ty) = self.expr_types.get(node_path) {
            return ty.clone();
        }

        // 2. Syntactic fallback.
        match expr {
            Expr::Literal(lit) => match lit {
                crate::ast::Literal::Int(_) => Type::I64,
                crate::ast::Literal::Float(_) => Type::F64,
                crate::ast::Literal::Str(_) => Type::Str,
                crate::ast::Literal::Bool(_) => Type::Bool,
            },
            Expr::None => Type::Option(Box::new(Type::Unknown)),
            Expr::Some(inner) => {
                let inner_ty =
                    self.resolve_expr_type(inner, &format!("{node_path}.inner"), scope);
                Type::Option(Box::new(inner_ty))
            }
            Expr::Ok(inner) => {
                let inner_ty =
                    self.resolve_expr_type(inner, &format!("{node_path}.inner"), scope);
                Type::Result(Box::new(inner_ty), Box::new(Type::Unknown))
            }
            Expr::Err(inner) => {
                let inner_ty =
                    self.resolve_expr_type(inner, &format!("{node_path}.inner"), scope);
                Type::Result(Box::new(Type::Unknown), Box::new(inner_ty))
            }
            Expr::Ident(name) => scope.get(name.as_str()).cloned().unwrap_or(Type::Unknown),
            Expr::Block(stmts) => {
                if let Option::Some(last) = stmts.last() {
                    self.resolve_expr_type(
                        &last.expr,
                        &format!("{node_path}.stmt_{}", stmts.len() - 1),
                        scope,
                    )
                } else {
                    Type::Unit
                }
            }
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name) = callee.as_ref() {
                    if let Option::Some(sig) = self.fn_sigs.get(name.as_str()) {
                        if type_contains_unresolved(&sig.ret) {
                            return Type::Unknown;
                        }
                        return sig.ret.clone();
                    }
                }
                Type::Unknown
            }
            Expr::Array(_) => Type::Slice(Box::new(Type::Unknown)),
            Expr::FmtStr { .. } => Type::Str,
            Expr::StructLit { name, .. } => {
                // Resolve struct literal type by looking up struct fields.
                // If the name contains "::" (e.g. "Expr::Lit"), it is an enum variant
                // struct literal — the resulting type is the parent enum, not a struct.
                // Without this, passing an enum variant literal as a function argument
                // would either produce a spurious E0306 (if the enum name happened to
                // appear in struct_fields) or silently skip the check (Type::Unknown).
                if name.contains("::") {
                    let enum_name = name.split("::").next().unwrap_or(name).to_string();
                    if self.known_enums.contains(&enum_name) {
                        return Type::Enum(enum_name);
                    }
                }
                let base_name = name.split("::").next().unwrap_or(name);
                if self.struct_fields.contains_key(base_name) {
                    Type::Struct(base_name.to_string())
                } else {
                    Type::Unknown
                }
            }
            _ => Type::Unknown,
        }
    }
}

// ── AxonType → Type conversion ────────────────────────────────────────────────

/// Convert an AST type annotation to a resolved `Type`.
///
/// This is a best-effort conversion: named types that the checker does not
/// know about become `Type::Struct(name)` so the R08 pass can flag them
/// independently.
pub fn axon_type_to_type(ty: &AxonType) -> Type {
    match ty {
        AxonType::Named(n) => match n.as_str() {
            "i8" => Type::I8,
            "i16" => Type::I16,
            "i32" => Type::I32,
            "i64" => Type::I64,
            "u8" => Type::U8,
            "u16" => Type::U16,
            "u32" => Type::U32,
            "u64" => Type::U64,
            "f32" => Type::F32,
            "f64" => Type::F64,
            "bool" => Type::Bool,
            "str" | "String" => Type::Str,
            "()" | "unit" => Type::Unit,
            other => {
                if DEFERRED_PREFIXES.iter().any(|p| other.starts_with(p)) {
                    Type::Deferred(other.to_string())
                } else {
                    Type::Struct(other.to_string())
                }
            }
        },
        AxonType::Result { ok, err } => {
            Type::Result(Box::new(axon_type_to_type(ok)), Box::new(axon_type_to_type(err)))
        }
        AxonType::Option(inner) => Type::Option(Box::new(axon_type_to_type(inner))),
        AxonType::Chan(inner) => Type::Chan(Box::new(axon_type_to_type(inner))),
        AxonType::Slice(inner) => Type::Slice(Box::new(axon_type_to_type(inner))),
        AxonType::Generic { base, args } => {
            if DEFERRED_PREFIXES.iter().any(|p| base.starts_with(p)) {
                return Type::Deferred(base.clone());
            }
            let _ = args;
            Type::Struct(base.clone())
        }
        AxonType::Fn { params, ret } => Type::Fn(
            params.iter().map(axon_type_to_type).collect(),
            Box::new(axon_type_to_type(ret)),
        ),
        AxonType::Ref(inner) => axon_type_to_type(inner),
        AxonType::TypeParam(name) => Type::TypeParam(name.clone()),
        AxonType::DynTrait(name) => Type::DynTrait(name.clone()),
        AxonType::Tuple(elems) => Type::Tuple(elems.iter().map(axon_type_to_type).collect()),
        // Union types are not yet first-class in the semantic type system.
        // Treat permissively as `Type::Unknown` to skip strict signature checks
        // (E0306 etc.) for union-typed arguments.
        AxonType::Union(_) => Type::Unknown,
    }
}

// ── AxonType helpers for E0501/E0502/E0503 ────────────────────────────────────

/// Returns a human-readable name for an `AxonType`, used in error messages.
fn axon_type_name(ty: &AxonType) -> String {
    match ty {
        AxonType::Named(n) => n.clone(),
        AxonType::Generic { base, args } => {
            if args.is_empty() {
                base.clone()
            } else {
                let args_str: Vec<String> = args.iter().map(axon_type_name).collect();
                format!("{}<{}>", base, args_str.join(", "))
            }
        }
        AxonType::Result { ok, err } => {
            format!("Result<{}, {}>", axon_type_name(ok), axon_type_name(err))
        }
        AxonType::Option(inner) => format!("Option<{}>", axon_type_name(inner)),
        AxonType::Chan(inner) => format!("Chan<{}>", axon_type_name(inner)),
        AxonType::Slice(inner) => format!("Slice<{}>", axon_type_name(inner)),
        AxonType::Fn { params, ret } => {
            let ps: Vec<String> = params.iter().map(axon_type_name).collect();
            format!("fn({}) -> {}", ps.join(", "), axon_type_name(ret))
        }
        AxonType::Ref(inner) => format!("&{}", axon_type_name(inner)),
        AxonType::DynTrait(n) => format!("dyn {n}"),
        AxonType::TypeParam(n) => n.clone(),
        AxonType::Tuple(elems) => {
            let parts: Vec<String> = elems.iter().map(axon_type_name).collect();
            format!("({})", parts.join(", "))
        }
        AxonType::Union(members) => {
            let parts: Vec<String> = members.iter().map(axon_type_name).collect();
            parts.join("|")
        }
    }
}

/// Returns a display string for an `AxonType` suitable for error messages.
/// Identical to `axon_type_name` for now; separated so they can diverge.
fn axon_type_display(ty: &AxonType) -> String {
    axon_type_name(ty)
}

/// Returns true if two `AxonType` annotations are compatible for signature
/// checking.  Named types must match exactly; generic containers recurse.
/// `TypeParam` matches anything (permissive for generic trait methods).
fn axon_types_compatible(a: &AxonType, b: &AxonType) -> bool {
    // A bare type parameter in the trait definition is compatible with anything.
    if matches!(a, AxonType::TypeParam(_)) || matches!(b, AxonType::TypeParam(_)) {
        return true;
    }
    // Union types are permissive (TS-style) — treated as compatible with anything
    // until the semantic type system supports proper union resolution.
    if matches!(a, AxonType::Union(_)) || matches!(b, AxonType::Union(_)) {
        return true;
    }
    // "Self" placeholder in trait signatures is compatible with any concrete type.
    if matches!(a, AxonType::Named(n) if n == "Self")
        || matches!(b, AxonType::Named(n) if n == "Self")
    {
        return true;
    }
    match (a, b) {
        (AxonType::Named(na), AxonType::Named(nb)) => na == nb,
        (AxonType::Generic { base: ba, args: aa }, AxonType::Generic { base: bb, args: ab }) => {
            ba == bb
                && aa.len() == ab.len()
                && aa.iter().zip(ab.iter()).all(|(x, y)| axon_types_compatible(x, y))
        }
        (AxonType::Result { ok: oa, err: ea }, AxonType::Result { ok: ob, err: eb }) => {
            axon_types_compatible(oa, ob) && axon_types_compatible(ea, eb)
        }
        (AxonType::Option(ia), AxonType::Option(ib)) => axon_types_compatible(ia, ib),
        (AxonType::Chan(ia), AxonType::Chan(ib)) => axon_types_compatible(ia, ib),
        (AxonType::Slice(ia), AxonType::Slice(ib)) => axon_types_compatible(ia, ib),
        (AxonType::Ref(ia), AxonType::Ref(ib)) => axon_types_compatible(ia, ib),
        (AxonType::DynTrait(na), AxonType::DynTrait(nb)) => na == nb,
        (
            AxonType::Fn { params: pa, ret: ra },
            AxonType::Fn { params: pb, ret: rb },
        ) => {
            pa.len() == pb.len()
                && pa.iter().zip(pb.iter()).all(|(x, y)| axon_types_compatible(x, y))
                && axon_types_compatible(ra, rb)
        }
        _ => false,
    }
}

// ── Levenshtein name suggestion (R08) ─────────────────────────────────────────

fn closest_name<'a>(name: &str, candidates: &'a [String]) -> Option<&'a str> {
    candidates
        .iter()
        .filter_map(|c| {
            let d = levenshtein(name, c);
            if d <= 3 { Option::Some((d, c.as_str())) } else { Option::None }
        })
        .min_by_key(|(d, _)| *d)
        .map(|(_, s)| s)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{
        AxonType, BinOp, EnumDef, EnumVariant, Expr, FnDef, Item, Literal, MatchArm, Pattern,
        Program, Stmt,
    };

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn make_program(items: Vec<Item>) -> Program {
        Program { items }
    }

    fn simple_fn(
        name: &str,
        params: Vec<crate::ast::Param>,
        return_type: Option<AxonType>,
        body: Expr,
    ) -> Item {
        Item::FnDef(FnDef {
            public: false,
            name: name.to_string(),
            generic_params: vec![],
            generic_bounds: vec![],
            params,
            return_type,
            body,
            attrs: vec![],
            contained: None,
            span: crate::span::Span::dummy(),
        })
    }

    fn param(name: &str, ty: AxonType) -> crate::ast::Param {
        crate::ast::Param { name: name.to_string(), ty, span: crate::span::Span::dummy() }
    }

    fn lit_int(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n))
    }

    fn lit_str(s: &str) -> Expr {
        Expr::Literal(Literal::Str(s.to_string()))
    }

    fn ident(s: &str) -> Expr {
        Expr::Ident(s.to_string())
    }

    fn block(stmts: Vec<Expr>) -> Expr {
        Expr::Block(stmts.into_iter().map(Stmt::simple).collect())
    }

    fn mk_ctx(fn_sigs: HashMap<String, FnSig>) -> CheckCtx {
        CheckCtx::new("test.ax", fn_sigs, HashMap::new())
    }

    fn run(ctx: &mut CheckCtx, program: &Program) -> Vec<CheckError> {
        ctx.check_program(program, HashMap::new())
    }

    fn run_with_types(
        ctx: &mut CheckCtx,
        program: &Program,
        expr_types: HashMap<String, Type>,
    ) -> Vec<CheckError> {
        ctx.check_program(program, expr_types)
    }

    // ── R01: Option<i32> passed to fn expecting i32 → E0301 ──────────────────

    #[test]
    fn r01_option_used_as_value() {
        // fn add_one(x: i32) -> i32 { x }
        // fn caller(opt_val: Option<i32>) -> i32 { add_one(opt_val) }
        let mut sigs = HashMap::new();
        sigs.insert(
            "add_one".to_string(),
            FnSig { params: vec![Type::I32], ret: Type::I32 },
        );

        let mut ctx = mk_ctx(sigs);

        let program = make_program(vec![simple_fn(
            "caller",
            vec![param("opt_val", AxonType::Option(Box::new(AxonType::Named("i32".into()))))],
            Option::Some(AxonType::Named("i32".into())),
            block(vec![Expr::Call {
                callee: Box::new(ident("add_one")),
                args: vec![ident("opt_val")],
            }]),
        )]);

        // Seed the arg node with Option<i32> so the checker sees it.
        let mut expr_types = HashMap::new();
        expr_types.insert(
            "#fn_caller.body.stmt_0.arg_0".to_string(),
            Type::Option(Box::new(Type::I32)),
        );
        // Return value matches i32 — suppress spurious R07.
        expr_types.insert("#fn_caller.body.stmt_0".to_string(), Type::I32);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        assert!(
            errors.iter().any(|e| e.code == E0301),
            "expected E0301, got: {errors:?}"
        );
    }

    // ── R02: fn returning Result called, result not used → E0302 ─────────────

    #[test]
    fn r02_result_ignored() {
        // fn may_fail() -> Result<i32, str> { ... }
        // fn caller() -> () { may_fail(); 0 }  ← result ignored
        let mut sigs = HashMap::new();
        sigs.insert(
            "may_fail".to_string(),
            FnSig {
                params: vec![],
                ret: Type::Result(Box::new(Type::I32), Box::new(Type::Str)),
            },
        );

        let mut ctx = mk_ctx(sigs);

        let program = make_program(vec![simple_fn(
            "caller",
            vec![],
            Option::Some(AxonType::Named("()".into())),
            // Block: stmt 0 (non-final) = may_fail(); stmt 1 (final) = 0
            block(vec![
                Expr::Call { callee: Box::new(ident("may_fail")), args: vec![] },
                lit_int(0),
            ]),
        )]);

        let errors = run(&mut ctx, &program);
        assert!(
            errors.iter().any(|e| e.code == E0302),
            "expected E0302, got: {errors:?}"
        );
    }

    // ── R03: ? operator in fn returning () → E0303 ────────────────────────────

    #[test]
    fn r03_question_in_unit_fn() {
        let mut ctx = mk_ctx(HashMap::new());

        // fn caller() -> () { x? }
        let program = make_program(vec![simple_fn(
            "caller",
            vec![],
            Option::Some(AxonType::Named("()".into())),
            block(vec![Expr::Question(Box::new(ident("x")))]),
        )]);

        let errors = run(&mut ctx, &program);
        assert!(
            errors.iter().any(|e| e.code == E0303),
            "expected E0303, got: {errors:?}"
        );
    }

    // ── R04: match on Option missing None arm → E0304 ─────────────────────────

    #[test]
    fn r04_match_option_missing_none() {
        let mut ctx = mk_ctx(HashMap::new());

        // fn f(x: Option<i32>) -> i32 {
        //   match x { Some(v) => v }   ← missing None arm
        // }
        let program = make_program(vec![simple_fn(
            "f",
            vec![param("x", AxonType::Option(Box::new(AxonType::Named("i32".into()))))],
            Option::Some(AxonType::Named("i32".into())),
            block(vec![Expr::Match {
                subject: Box::new(ident("x")),
                arms: vec![MatchArm {
                    pattern: Pattern::Some(Box::new(Pattern::Ident("v".into()))),
                    guard: Option::None,
                    body: ident("v"),
                }],
            }]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert(
            "#fn_f.body.stmt_0.subject".to_string(),
            Type::Option(Box::new(Type::I32)),
        );
        // Suppress R07 for the match expression result.
        expr_types.insert("#fn_f.body.stmt_0".to_string(), Type::I32);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        assert!(
            errors.iter().any(|e| e.code == E0304 && e.message.contains("None")),
            "expected E0304 (missing None), got: {errors:?}"
        );
    }

    // ── R05: fn called with wrong number of args → E0305 ─────────────────────

    #[test]
    fn r05_wrong_arg_count() {
        let mut sigs = HashMap::new();
        sigs.insert(
            "two_arg".to_string(),
            FnSig {
                params: vec![Type::I32, Type::I32],
                ret: Type::I32,
            },
        );
        let mut ctx = mk_ctx(sigs);

        // fn caller() -> i32 { two_arg(1) }  ← 1 arg, expects 2
        let program = make_program(vec![simple_fn(
            "caller",
            vec![],
            Option::Some(AxonType::Named("i32".into())),
            block(vec![Expr::Call {
                callee: Box::new(ident("two_arg")),
                args: vec![lit_int(1)],
            }]),
        )]);

        let errors = run(&mut ctx, &program);
        assert!(
            errors.iter().any(|e| e.code == E0305),
            "expected E0305, got: {errors:?}"
        );
    }

    // ── R06: fn called with wrong arg type → E0306 ────────────────────────────

    #[test]
    fn r06_wrong_arg_type() {
        let mut sigs = HashMap::new();
        sigs.insert(
            "wants_bool".to_string(),
            FnSig { params: vec![Type::Bool], ret: Type::Unit },
        );
        let mut ctx = mk_ctx(sigs);

        // fn caller() -> () { wants_bool(42); true }
        let program = make_program(vec![simple_fn(
            "caller",
            vec![],
            Option::Some(AxonType::Named("()".into())),
            block(vec![
                // non-final: wants_bool(42)
                Expr::Call {
                    callee: Box::new(ident("wants_bool")),
                    args: vec![lit_int(42)], // i64 ≠ bool → E0306
                },
                // final: bool to keep R07 happy with () return type
                Expr::Literal(Literal::Bool(true)),
            ]),
        )]);

        let mut expr_types = HashMap::new();
        // Stamp non-final call as Unit so R02 doesn't fire.
        expr_types.insert("#fn_caller.body.stmt_0".to_string(), Type::Unit);
        // Final bool → () for R07.
        expr_types.insert("#fn_caller.body.stmt_1".to_string(), Type::Unit);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        assert!(
            errors.iter().any(|e| e.code == E0306),
            "expected E0306, got: {errors:?}"
        );
    }

    // ── R07: fn declares ->i32 but returns str → E0307 ───────────────────────

    #[test]
    fn r07_return_type_mismatch() {
        let mut ctx = mk_ctx(HashMap::new());

        // fn f() -> i32 { "hello" }
        let program = make_program(vec![simple_fn(
            "f",
            vec![],
            Option::Some(AxonType::Named("i32".into())),
            block(vec![lit_str("hello")]),
        )]);

        let errors = run(&mut ctx, &program);
        assert!(
            errors.iter().any(|e| e.code == E0307),
            "expected E0307, got: {errors:?}"
        );
    }

    // ── R08: type annotation uses unknown type name → E0308 ──────────────────

    #[test]
    fn r08_unknown_type() {
        let mut ctx = mk_ctx(HashMap::new());

        // fn f(x: Flibbertigibbet) -> i32 { 0 }
        let program = make_program(vec![simple_fn(
            "f",
            vec![param("x", AxonType::Named("Flibbertigibbet".into()))],
            Option::Some(AxonType::Named("i32".into())),
            block(vec![lit_int(0)]),
        )]);

        let errors = run(&mut ctx, &program);
        assert!(
            errors.iter().any(|e| e.code == E0308),
            "expected E0308, got: {errors:?}"
        );
    }

    // ── R11: field access on i32 → E0309 ─────────────────────────────────────

    #[test]
    fn r11_field_access_on_non_struct() {
        let mut ctx = mk_ctx(HashMap::new());

        // fn f() -> i32 { let x = 42; x.foo }
        let program = make_program(vec![simple_fn(
            "f",
            vec![],
            Option::Some(AxonType::Named("i32".into())),
            block(vec![
                Expr::Let { name: "x".into(), value: Box::new(lit_int(42)) },
                Expr::FieldAccess { receiver: Box::new(ident("x")), field: "foo".into() },
            ]),
        )]);

        // Tell the checker the receiver has type i32.
        let mut expr_types = HashMap::new();
        expr_types.insert("#fn_f.body.stmt_1.receiver".to_string(), Type::I32);
        // Suppress R07: stamp the field access result as i32.
        expr_types.insert("#fn_f.body.stmt_1".to_string(), Type::I32);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        assert!(
            errors.iter().any(|e| e.code == E0401 || e.code == E0309),
            "expected E0401 (or E0309), got: {errors:?}"
        );
    }

    // ── R12: Deferred type passes silently → no E0306 / E0301 ────────────────

    #[test]
    fn r12_deferred_type_silent() {
        let mut sigs = HashMap::new();
        sigs.insert(
            "takes_i32".to_string(),
            FnSig { params: vec![Type::I32], ret: Type::Unit },
        );
        let mut ctx = mk_ctx(sigs);

        // fn f(x: Uncertain<i32>) -> () { takes_i32(x); true }
        let program = make_program(vec![simple_fn(
            "f",
            vec![param("x", AxonType::Named("Uncertain<i32>".into()))],
            Option::Some(AxonType::Named("()".into())),
            block(vec![
                Expr::Call {
                    callee: Box::new(ident("takes_i32")),
                    args: vec![ident("x")],
                },
                Expr::Literal(Literal::Bool(true)),
            ]),
        )]);

        let mut expr_types = HashMap::new();
        // Stamp the arg as Deferred — checker must skip R06/R01.
        expr_types.insert(
            "#fn_f.body.stmt_0.arg_0".to_string(),
            Type::Deferred("Uncertain<i32>".into()),
        );
        expr_types.insert("#fn_f.body.stmt_0".to_string(), Type::Unit);
        // Suppress R07: the final bool maps to ().
        expr_types.insert("#fn_f.body.stmt_1".to_string(), Type::Unit);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        let type_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.code == E0306 || e.code == E0301)
            .collect();
        assert!(
            type_errors.is_empty(),
            "expected no E0306/E0301 for Deferred arg, got: {type_errors:?}"
        );
    }

    // ── R04 extra: match on Result missing Err arm → E0304 ───────────────────

    #[test]
    fn r04_match_result_missing_err() {
        let mut ctx = mk_ctx(HashMap::new());

        let program = make_program(vec![simple_fn(
            "f",
            vec![param(
                "r",
                AxonType::Result {
                    ok: Box::new(AxonType::Named("i32".into())),
                    err: Box::new(AxonType::Named("str".into())),
                },
            )],
            Option::Some(AxonType::Named("i32".into())),
            block(vec![Expr::Match {
                subject: Box::new(ident("r")),
                arms: vec![MatchArm {
                    pattern: Pattern::Ok(Box::new(Pattern::Ident("v".into()))),
                    guard: Option::None,
                    body: ident("v"),
                }],
            }]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert(
            "#fn_f.body.stmt_0.subject".to_string(),
            Type::Result(Box::new(Type::I32), Box::new(Type::Str)),
        );
        expr_types.insert("#fn_f.body.stmt_0".to_string(), Type::I32);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        assert!(
            errors.iter().any(|e| e.code == E0304 && e.message.contains("Err")),
            "expected E0304 (missing Err), got: {errors:?}"
        );
    }

    // ── R03 extra: ? inside Result-returning fn is fine ───────────────────────

    #[test]
    fn r03_question_in_result_fn_ok() {
        let mut ctx = mk_ctx(HashMap::new());

        // fn f() -> Result<i32, str> { x? }
        let program = make_program(vec![simple_fn(
            "f",
            vec![],
            Option::Some(AxonType::Result {
                ok: Box::new(AxonType::Named("i32".into())),
                err: Box::new(AxonType::Named("str".into())),
            }),
            block(vec![Expr::Question(Box::new(ident("x")))]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert(
            "#fn_f.body.stmt_0".to_string(),
            Type::Result(Box::new(Type::I32), Box::new(Type::Str)),
        );

        let errors = run_with_types(&mut ctx, &program, expr_types);
        let r03_errors: Vec<_> = errors.iter().filter(|e| e.code == E0303).collect();
        assert!(
            r03_errors.is_empty(),
            "expected no E0303 in Result-returning fn, got: {r03_errors:?}"
        );
    }

    // ── R02 extra: assigned Result is not ignored ─────────────────────────────

    #[test]
    fn r02_assigned_result_not_ignored() {
        let mut sigs = HashMap::new();
        sigs.insert(
            "may_fail".to_string(),
            FnSig {
                params: vec![],
                ret: Type::Result(Box::new(Type::I32), Box::new(Type::Str)),
            },
        );
        let mut ctx = mk_ctx(sigs);

        // fn f() -> () { let r = may_fail(); }
        let program = make_program(vec![simple_fn(
            "f",
            vec![],
            Option::Some(AxonType::Named("()".into())),
            block(vec![Expr::Let {
                name: "r".into(),
                value: Box::new(Expr::Call {
                    callee: Box::new(ident("may_fail")),
                    args: vec![],
                }),
            }]),
        )]);

        let errors = run(&mut ctx, &program);
        let r02: Vec<_> = errors.iter().filter(|e| e.code == E0302).collect();
        assert!(
            r02.is_empty(),
            "E0302 should not fire for assigned Result, got: {r02:?}"
        );
    }

    // ── R01 via BinOp: Option<i32> + i32 ─────────────────────────────────────

    #[test]
    fn r01_option_in_binop() {
        let mut ctx = mk_ctx(HashMap::new());

        // fn f(x: Option<i32>) -> i32 { x + 1 }
        let program = make_program(vec![simple_fn(
            "f",
            vec![param("x", AxonType::Option(Box::new(AxonType::Named("i32".into()))))],
            Option::Some(AxonType::Named("i32".into())),
            block(vec![Expr::BinOp {
                op: BinOp::Add,
                left: Box::new(ident("x")),
                right: Box::new(lit_int(1)),
            }]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert(
            "#fn_f.body.stmt_0.left".to_string(),
            Type::Option(Box::new(Type::I32)),
        );
        // Result of binop is i32 — matches return type.
        expr_types.insert("#fn_f.body.stmt_0".to_string(), Type::I32);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        assert!(
            errors.iter().any(|e| e.code == E0301),
            "expected E0301 for Option in BinOp, got: {errors:?}"
        );
    }

    // ── Fix #4: arithmetic on non-numeric type emits error ────────────────────

    #[test]
    fn fix4_arithmetic_on_str_errors() {
        let mut ctx = mk_ctx(HashMap::new());

        // fn f(s: str) -> i64 { s + 1 }
        let program = make_program(vec![simple_fn(
            "f",
            vec![param("s", AxonType::Named("str".into()))],
            Option::Some(AxonType::Named("i64".into())),
            block(vec![Expr::BinOp {
                op: BinOp::Add,
                left: Box::new(ident("s")),
                right: Box::new(lit_int(1)),
            }]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert("#fn_f.body.stmt_0.left".to_string(), Type::Str);
        expr_types.insert("#fn_f.body.stmt_0.right".to_string(), Type::I64);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        assert!(
            !errors.is_empty(),
            "expected error for str + i64 arithmetic, got none"
        );
    }

    #[test]
    fn fix4_arithmetic_on_numeric_ok() {
        let mut ctx = mk_ctx(HashMap::new());

        // fn f(x: i64) -> i64 { x + 1 }
        let program = make_program(vec![simple_fn(
            "f",
            vec![param("x", AxonType::Named("i64".into()))],
            Option::Some(AxonType::Named("i64".into())),
            block(vec![Expr::BinOp {
                op: BinOp::Add,
                left: Box::new(ident("x")),
                right: Box::new(lit_int(1)),
            }]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert("#fn_f.body.stmt_0.left".to_string(), Type::I64);
        expr_types.insert("#fn_f.body.stmt_0.right".to_string(), Type::I64);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        let arith_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.message.contains("non-numeric"))
            .collect();
        assert!(
            arith_errors.is_empty(),
            "i64 arithmetic should be error-free, got: {arith_errors:?}"
        );
    }

    #[test]
    fn fix4_modulo_on_bool_errors() {
        let mut ctx = mk_ctx(HashMap::new());

        // fn f(b: bool) -> i64 { b % 2 }
        let program = make_program(vec![simple_fn(
            "f",
            vec![param("b", AxonType::Named("bool".into()))],
            Option::Some(AxonType::Named("i64".into())),
            block(vec![Expr::BinOp {
                op: BinOp::Rem,
                left: Box::new(ident("b")),
                right: Box::new(lit_int(2)),
            }]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert("#fn_f.body.stmt_0.left".to_string(), Type::Bool);
        expr_types.insert("#fn_f.body.stmt_0.right".to_string(), Type::I64);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        assert!(
            !errors.is_empty(),
            "expected error for bool % i64, got none"
        );
    }

    // ── Integer widening at call sites: i32 arg to i64 param is OK ───────────

    #[test]
    fn r06_i32_to_i64_widening_is_ok() {
        let mut sigs = HashMap::new();
        sigs.insert(
            "wants_i64".to_string(),
            FnSig { params: vec![Type::I64], ret: Type::Unit },
        );
        let mut ctx = mk_ctx(sigs);

        // fn caller() -> () { wants_i64(x_i32) }
        // where x_i32 has type i32 — should NOT produce E0306
        let program = make_program(vec![simple_fn(
            "caller",
            vec![param("x", AxonType::Named("i32".into()))],
            Option::Some(AxonType::Named("()".into())),
            block(vec![
                Expr::Call {
                    callee: Box::new(ident("wants_i64")),
                    args: vec![ident("x")],
                },
                Expr::Literal(Literal::Bool(true)),
            ]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert("#fn_caller.body.stmt_0.arg_0".to_string(), Type::I32);
        expr_types.insert("#fn_caller.body.stmt_0".to_string(), Type::Unit);
        expr_types.insert("#fn_caller.body.stmt_1".to_string(), Type::Unit);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        let r06_errors: Vec<_> = errors.iter().filter(|e| e.code == E0306).collect();
        assert!(
            r06_errors.is_empty(),
            "i32→i64 widening should not produce E0306, got: {r06_errors:?}"
        );
    }

    #[test]
    fn r06_bool_to_i64_no_widening() {
        let mut sigs = HashMap::new();
        sigs.insert(
            "wants_i64".to_string(),
            FnSig { params: vec![Type::I64], ret: Type::Unit },
        );
        let mut ctx = mk_ctx(sigs);

        // fn caller() -> () { wants_i64(true) } — bool→i64 is not widening
        let program = make_program(vec![simple_fn(
            "caller",
            vec![],
            Option::Some(AxonType::Named("()".into())),
            block(vec![
                Expr::Call {
                    callee: Box::new(ident("wants_i64")),
                    args: vec![Expr::Literal(Literal::Bool(true))],
                },
                Expr::Literal(Literal::Bool(false)),
            ]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert("#fn_caller.body.stmt_0.arg_0".to_string(), Type::Bool);
        expr_types.insert("#fn_caller.body.stmt_0".to_string(), Type::Unit);
        expr_types.insert("#fn_caller.body.stmt_1".to_string(), Type::Unit);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        assert!(
            errors.iter().any(|e| e.code == E0306),
            "bool→i64 should produce E0306 (not a widening), got: {errors:?}"
        );
    }

    // ── Generic functions ────────────────────────────────────────────────────

    #[test]
    fn generic_fn_type_param_not_flagged_as_unknown() {
        // `fn identity<T>(x: T) -> T { x }` — `T` must not produce E0308.
        use crate::ast::{AxonType, Expr};
        let body = Expr::Ident("x".into());
        let fndef = Item::FnDef(FnDef {
            public: false,
            name: "identity".into(),
            generic_params: vec!["T".into()],
            generic_bounds: vec![],
            params: vec![param("x", AxonType::Named("T".into()))],
            return_type: Some(AxonType::Named("T".into())),
            body,
            attrs: vec![],
            contained: None,
            span: crate::span::Span::dummy(),
        });
        let program = make_program(vec![fndef]);
        let mut ctx = mk_ctx(HashMap::new());
        let errors = run_with_types(&mut ctx, &program, HashMap::new());
        let r08: Vec<_> = errors.iter().filter(|e| e.code == E0308).collect();
        assert!(r08.is_empty(), "generic param T should not produce E0308: {r08:?}");
    }

    // ── Trait validation (E0501 / E0502 / E0503) ─────────────────────────────

    fn make_trait_def(name: &str, methods: Vec<crate::ast::TraitMethod>) -> Item {
        Item::TraitDef(crate::ast::TraitDef {
            name: name.to_string(),
            generic_params: vec![],
            methods,
            span: crate::span::Span::dummy(),
        })
    }

    fn make_trait_method(
        name: &str,
        params: Vec<crate::ast::Param>,
        return_type: Option<AxonType>,
    ) -> crate::ast::TraitMethod {
        crate::ast::TraitMethod {
            name: name.to_string(),
            params,
            return_type,
            span: crate::span::Span::dummy(),
        }
    }

    fn make_impl_block(
        trait_name: &str,
        for_type: AxonType,
        methods: Vec<FnDef>,
    ) -> Item {
        Item::ImplBlock(crate::ast::ImplBlock {
            trait_name: trait_name.to_string(),
            for_type,
            methods,
            generic_params: vec![],
            generic_bounds: vec![],
            span: crate::span::Span::dummy(),
        })
    }

    fn make_fndef(name: &str, params: Vec<crate::ast::Param>, ret: Option<AxonType>) -> FnDef {
        FnDef {
            public: false,
            name: name.to_string(),
            generic_params: vec![],
            generic_bounds: vec![],
            params,
            return_type: ret,
            body: lit_int(0),
            attrs: vec![],
            contained: None,
            span: crate::span::Span::dummy(),
        }
    }

    // ── E0504: trait bound not satisfied ─────────────────────────────────────

    #[test]
    fn e0504_bound_not_satisfied() {
        // fn show<T: Display>(x: T) — call with Qux which does NOT impl Display.
        // We need: (1) FnDef with bounds, (2) fn_sig with TypeParam param, (3) a call.
        use crate::types::Type;

        // Build fn show<T: Display>(x: T) signature in fn_sigs.
        let mut sigs = HashMap::new();
        sigs.insert(
            "show".to_string(),
            FnSig { params: vec![Type::TypeParam("T".into())], ret: Type::Unit },
        );
        let mut ctx = mk_ctx(sigs);

        // Register the bound on "show".
        ctx.fn_bounds.insert("show".into(), vec![("T".into(), vec!["Display".into()])]);
        // Qux does NOT implement Display (no impl entry).

        // fn caller() { show(qux_val) } where qux_val: Qux
        let program = make_program(vec![simple_fn(
            "caller",
            vec![],
            None,
            block(vec![
                Expr::Call {
                    callee: Box::new(ident("show")),
                    args: vec![ident("qux_val")],
                },
                lit_int(0),
            ]),
        )]);

        // Inject arg type into expr_types so resolve_expr_type returns Struct("Qux").
        let mut expr_types = HashMap::new();
        expr_types.insert("#fn_caller.body.stmt_0.arg_0".to_string(), Type::Struct("Qux".into()));
        expr_types.insert("#fn_caller.body.stmt_0".to_string(), Type::Unit);
        expr_types.insert("#fn_caller.body.stmt_1".to_string(), Type::I64);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        assert!(
            errors.iter().any(|e| e.code == E0504),
            "expected E0504 when type doesn't satisfy bound, got: {errors:?}"
        );
    }

    #[test]
    fn e0504_bound_satisfied_no_error() {
        // Same setup but Qux DOES implement Display.
        use crate::types::Type;

        let mut sigs = HashMap::new();
        sigs.insert(
            "show".to_string(),
            FnSig { params: vec![Type::TypeParam("T".into())], ret: Type::Unit },
        );
        let mut ctx = mk_ctx(sigs);
        ctx.fn_bounds.insert("show".into(), vec![("T".into(), vec!["Display".into()])]);
        // Register Qux as implementing Display.
        ctx.impl_table.entry("Qux".into()).or_default().insert("Display".into());

        let program = make_program(vec![simple_fn(
            "caller",
            vec![],
            None,
            block(vec![
                Expr::Call {
                    callee: Box::new(ident("show")),
                    args: vec![ident("qux_val")],
                },
                lit_int(0),
            ]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert("#fn_caller.body.stmt_0.arg_0".to_string(), Type::Struct("Qux".into()));
        expr_types.insert("#fn_caller.body.stmt_0".to_string(), Type::Unit);
        expr_types.insert("#fn_caller.body.stmt_1".to_string(), Type::I64);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        let e504: Vec<_> = errors.iter().filter(|e| e.code == E0504).collect();
        assert!(e504.is_empty(), "Qux implements Display, should not produce E0504: {e504:?}");
    }

    #[test]
    fn e0501_unknown_trait() {
        // impl of a trait that doesn't exist in the program.
        let program = make_program(vec![
            make_impl_block(
                "NonExistent",
                AxonType::Named("Foo".into()),
                vec![make_fndef("hello", vec![], None)],
            ),
        ]);
        let mut ctx = mk_ctx(HashMap::new());
        let errors = run_with_types(&mut ctx, &program, HashMap::new());
        assert!(
            errors.iter().any(|e| e.code == E0501),
            "expected E0501 for unknown trait, got: {errors:?}"
        );
    }

    #[test]
    fn e0502_missing_method() {
        // impl block omits `farewell` which the trait requires.
        let trait_def = make_trait_def("Greet", vec![
            make_trait_method("hello", vec![param("name", AxonType::Named("str".into()))], Some(AxonType::Named("str".into()))),
            make_trait_method("farewell", vec![], Some(AxonType::Named("str".into()))),
        ]);
        let impl_block = make_impl_block(
            "Greet",
            AxonType::Named("Bar".into()),
            vec![make_fndef(
                "hello",
                vec![param("name", AxonType::Named("str".into()))],
                Some(AxonType::Named("str".into())),
            )],
        );
        let program = make_program(vec![trait_def, impl_block]);
        let mut ctx = mk_ctx(HashMap::new());
        let errors = run_with_types(&mut ctx, &program, HashMap::new());
        assert!(
            errors.iter().any(|e| e.code == E0502 && e.message.contains("farewell")),
            "expected E0502 for missing 'farewell', got: {errors:?}"
        );
    }

    #[test]
    fn e0503_param_count_mismatch() {
        // impl method has 0 params; trait declares 1 param.
        let trait_def = make_trait_def("Greet", vec![
            make_trait_method("hello", vec![param("name", AxonType::Named("str".into()))], Some(AxonType::Named("str".into()))),
        ]);
        let impl_block = make_impl_block(
            "Greet",
            AxonType::Named("Baz".into()),
            vec![make_fndef("hello", vec![], Some(AxonType::Named("str".into())))],
        );
        let program = make_program(vec![trait_def, impl_block]);
        let mut ctx = mk_ctx(HashMap::new());
        let errors = run_with_types(&mut ctx, &program, HashMap::new());
        assert!(
            errors.iter().any(|e| e.code == E0503),
            "expected E0503 for param count mismatch, got: {errors:?}"
        );
    }

    #[test]
    fn valid_impl_produces_no_trait_errors() {
        // A complete, correct impl produces no E0501/E0502/E0503.
        let trait_def = make_trait_def("Greet", vec![
            make_trait_method("hello", vec![param("name", AxonType::Named("str".into()))], Some(AxonType::Named("str".into()))),
        ]);
        let impl_block = make_impl_block(
            "Greet",
            AxonType::Named("Qux".into()),
            vec![make_fndef(
                "hello",
                vec![param("name", AxonType::Named("str".into()))],
                Some(AxonType::Named("str".into())),
            )],
        );
        let program = make_program(vec![trait_def, impl_block]);
        let mut ctx = mk_ctx(HashMap::new());
        let errors = run_with_types(&mut ctx, &program, HashMap::new());
        let trait_errors: Vec<_> = errors.iter()
            .filter(|e| e.code == E0501 || e.code == E0502 || e.code == E0503)
            .collect();
        assert!(trait_errors.is_empty(), "valid impl should not produce trait errors, got: {trait_errors:?}");
    }

    // ── Diagnostic-quality tests (improved messages) ─────────────────────────
    //
    // These tests pin the substrings of the rewritten user-facing messages so
    // future refactors don't silently regress diagnostic clarity.

    /// Helper: build a `Stmt` with a non-dummy span so the checker can attach
    /// source location info to errors raised inside it.
    fn stmt_with_span(expr: Expr, span: crate::span::Span) -> Stmt {
        Stmt { expr, span }
    }

    #[test]
    fn e0305_message_names_function_and_signature() {
        let mut sigs = HashMap::new();
        sigs.insert(
            "two_arg".to_string(),
            FnSig { params: vec![Type::I32, Type::Bool], ret: Type::I32 },
        );
        let mut ctx = mk_ctx(sigs);

        let program = make_program(vec![simple_fn(
            "caller",
            vec![],
            Option::Some(AxonType::Named("i32".into())),
            block(vec![Expr::Call {
                callee: Box::new(ident("two_arg")),
                args: vec![lit_int(1)],
            }]),
        )]);

        let errors = run(&mut ctx, &program);
        let e0305 = errors.iter().find(|e| e.code == E0305)
            .expect("expected E0305");
        // New message: "function `two_arg` takes 2 arguments but 1 was supplied"
        assert!(e0305.message.contains("`two_arg`"),
            "E0305 message should name the function: {}", e0305.message);
        assert!(e0305.message.contains("2 arguments"),
            "E0305 message should state expected arity: {}", e0305.message);
        assert!(e0305.message.contains("1 was supplied"),
            "E0305 message should state observed arity: {}", e0305.message);
        // Fix should spell out the signature.
        let fix = e0305.fix.as_deref().unwrap_or("");
        assert!(fix.contains("two_arg") && fix.contains("i32") && fix.contains("bool"),
            "E0305 fix should render the expected signature: {fix}");
    }

    #[test]
    fn e0306_message_names_arg_index_and_types() {
        let mut sigs = HashMap::new();
        sigs.insert(
            "wants_bool".to_string(),
            FnSig { params: vec![Type::Bool], ret: Type::Unit },
        );
        let mut ctx = mk_ctx(sigs);

        let program = make_program(vec![simple_fn(
            "caller",
            vec![],
            Option::Some(AxonType::Named("()".into())),
            block(vec![
                Expr::Call {
                    callee: Box::new(ident("wants_bool")),
                    args: vec![lit_int(42)],
                },
                Expr::Literal(Literal::Bool(true)),
            ]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert("#fn_caller.body.stmt_0".to_string(), Type::Unit);
        expr_types.insert("#fn_caller.body.stmt_1".to_string(), Type::Unit);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        let e0306 = errors.iter().find(|e| e.code == E0306)
            .expect("expected E0306");
        assert!(e0306.message.contains("argument 0"),
            "E0306 message should pinpoint argument index: {}", e0306.message);
        assert!(e0306.message.contains("`wants_bool`"),
            "E0306 message should name the function: {}", e0306.message);
        assert!(e0306.message.contains("expected `bool`"),
            "E0306 message should spell expected: {}", e0306.message);
        assert!(e0306.message.contains("found `i64`"),
            "E0306 message should spell found: {}", e0306.message);
    }

    #[test]
    fn e0307_return_mismatch_suggests_ok_wrap() {
        let mut ctx = mk_ctx(HashMap::new());

        // fn f() -> Result<i32, str> { 42 } — should suggest `Ok(...)`.
        let program = make_program(vec![simple_fn(
            "f",
            vec![],
            Option::Some(AxonType::Result {
                ok: Box::new(AxonType::Named("i32".into())),
                err: Box::new(AxonType::Named("str".into())),
            }),
            block(vec![lit_int(0)]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert("#fn_f.body.stmt_0".to_string(), Type::I32);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        let e0307 = errors.iter().find(|e| e.code == E0307)
            .expect("expected E0307");
        assert!(e0307.message.contains("return type mismatch"),
            "E0307 should phrase the message clearly: {}", e0307.message);
        let fix = e0307.fix.as_deref().unwrap_or("");
        assert!(fix.contains("Ok("),
            "E0307 fix should suggest `Ok(...)` wrapping: {fix}");
    }

    #[test]
    fn e0303_message_mentions_result_and_actual_return_type() {
        let mut ctx = mk_ctx(HashMap::new());

        // fn f() -> i32 { x? }
        let program = make_program(vec![simple_fn(
            "f",
            vec![],
            Option::Some(AxonType::Named("i32".into())),
            block(vec![Expr::Question(Box::new(ident("x")))]),
        )]);

        let errors = run(&mut ctx, &program);
        let e0303 = errors.iter().find(|e| e.code == E0303)
            .expect("expected E0303");
        assert!(e0303.message.contains("`?`"),
            "E0303 should mention the `?` operator: {}", e0303.message);
        assert!(e0303.message.contains("Result"),
            "E0303 should mention Result: {}", e0303.message);
        let fix = e0303.fix.as_deref().unwrap_or("");
        assert!(fix.contains("Result") || fix.contains("match"),
            "E0303 fix should guide the user: {fix}");
    }

    #[test]
    fn e0301_message_names_inner_type() {
        // R01 via BinOp: Option<i32> + i32 — checker emits E0301 with the
        // wrapped type spelt out.
        let mut ctx = mk_ctx(HashMap::new());

        let program = make_program(vec![simple_fn(
            "f",
            vec![param("x", AxonType::Option(Box::new(AxonType::Named("i32".into()))))],
            Option::Some(AxonType::Named("i32".into())),
            block(vec![Expr::BinOp {
                op: BinOp::Add,
                left: Box::new(ident("x")),
                right: Box::new(lit_int(1)),
            }]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert("#fn_f.body.stmt_0.left".to_string(),
            Type::Option(Box::new(Type::I32)));
        expr_types.insert("#fn_f.body.stmt_0".to_string(), Type::I32);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        let e0301 = errors.iter().find(|e| e.code == E0301)
            .expect("expected E0301");
        assert!(e0301.message.contains("Option<i32>"),
            "E0301 should spell out Option<inner>: {}", e0301.message);
        let fix = e0301.fix.as_deref().unwrap_or("");
        assert!(fix.contains("unwrap_or") || fix.contains("match"),
            "E0301 fix should suggest unwrap or match: {fix}");
    }

    #[test]
    fn span_propagates_to_e0305_when_stmt_has_span() {
        // When the enclosing Stmt carries a non-dummy span, the checker should
        // attach it to the diagnostic so lib.rs can render `file:line:col`.
        let mut sigs = HashMap::new();
        sigs.insert(
            "two_arg".to_string(),
            FnSig { params: vec![Type::I32, Type::I32], ret: Type::I32 },
        );
        let mut ctx = mk_ctx(sigs);

        // Build a block where the call statement has a real span.
        let call_expr = Expr::Call {
            callee: Box::new(ident("two_arg")),
            args: vec![lit_int(1)],
        };
        let body = Expr::Block(vec![stmt_with_span(call_expr, crate::span::Span::new(15, 28))]);

        let program = make_program(vec![simple_fn(
            "caller",
            vec![],
            Option::Some(AxonType::Named("i32".into())),
            body,
        )]);

        let errors = run(&mut ctx, &program);
        let e0305 = errors.iter().find(|e| e.code == E0305)
            .expect("expected E0305");
        assert!(!e0305.span.is_dummy(),
            "E0305 should carry the statement's span (was dummy)");
        assert_eq!(e0305.span.start, 15,
            "E0305 span should match the surrounding statement's start");
    }

    // ── R06: enum variant struct literal passed to enum-typed param → no E0306 ─

    /// Passing `EnumName::Variant { field: val }` to a function that expects
    /// `EnumName` must NOT produce a spurious E0306.
    ///
    /// Root cause: `resolve_expr_type` for `Expr::StructLit` must return
    /// `Type::Enum(enum_name)` (not `Type::Unknown` or `Type::Struct(...)`)
    /// when the literal name contains "::" and the prefix is a known enum.
    #[test]
    fn r06_enum_variant_struct_lit_no_false_positive() {
        // Build a CheckCtx that knows about `eval(e: Expr) -> i64` and the
        // enum `Expr` with variant `Lit`.
        let mut sigs = HashMap::new();
        sigs.insert(
            "eval".to_string(),
            FnSig { params: vec![Type::Enum("Expr".into())], ret: Type::I64 },
        );
        // CheckCtx needs no struct_fields for this test (Expr is an enum).
        let mut ctx = mk_ctx(sigs); // mut needed by run_with_types

        // Program:
        //   enum Expr { Lit { value: i64 } }
        //   fn caller() -> i64 { eval(Expr::Lit { value: 42 }) }
        let enum_item = Item::EnumDef(EnumDef {
            name: "Expr".into(),
            generic_params: vec![],
            variants: vec![EnumVariant {
                name: "Lit".into(),
                fields: vec![crate::ast::TypeField {
                    name: "value".into(),
                    ty: AxonType::Named("i64".into()),
                }],
            }],
            span: crate::span::Span::dummy(),
        });
        let caller_fn = simple_fn(
            "caller",
            vec![],
            Option::Some(AxonType::Named("i64".into())),
            block(vec![
                // eval(Expr::Lit { value: 42 })
                Expr::Call {
                    callee: Box::new(ident("eval")),
                    args: vec![Expr::StructLit {
                        name: "Expr::Lit".into(),
                        fields: vec![("value".into(), lit_int(42))],
                    }],
                },
            ]),
        );
        let program = make_program(vec![enum_item, caller_fn]);

        // Stamp the call expression as i64 so R02/R07 don't interfere.
        let mut expr_types = HashMap::new();
        expr_types.insert("#fn_caller.body.stmt_0".to_string(), Type::I64);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        let e0306_errors: Vec<_> = errors.iter().filter(|e| e.code == E0306).collect();
        assert!(
            e0306_errors.is_empty(),
            "enum variant struct literal passed to enum-typed param should not produce E0306, \
             got: {e0306_errors:?}"
        );
    }
}
