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

use crate::ast::{AxonType, Expr, FmtPart, FnDef, Item, MatchArm, Pattern, Program, Stmt};
use crate::error::{
    levenshtein, E1206, E1207, E1208, E1209, E1210, E1500, E1503, E1504, E1505, E1704, E1810,
    E2300, E2302,
};
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
pub const E0402: &str = "E0402"; // indexing a non-indexable (non-array) type
pub const E0403: &str = "E0403"; // calling a data field as a method (`p.x()`)
pub const E0404: &str = "E0404"; // enum-variant literal names a nonexistent variant
pub const E0405: &str = "E0405"; // literal pattern's type can't match the match subject
pub const E0406: &str = "E0406"; // a field is set more than once in a struct literal
pub const E0407: &str = "E0407"; // integer division/remainder by a literal zero

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

    pub fn warning(code: &'static str, message: impl Into<String>) -> Self {
        let mut e = Self::new(code, message);
        e.severity = Severity::Warning;
        e
    }

    pub fn info(code: &'static str, message: impl Into<String>) -> Self {
        let mut e = Self::new(code, message);
        e.severity = Severity::Info;
        e
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
        Type::Result(ok, err) => type_contains_unresolved(ok) || type_contains_unresolved(err),
        Type::Tuple(elems) => elems.iter().any(type_contains_unresolved),
        _ => false,
    }
}

/// True when a builtin signature's `Type::Deferred(name)` is a *type parameter*
/// slot (`T`, `U`, `V`, `K`, `E`) rather than an opaque deferred type (`Dict`,
/// `Uncertain<…>`) or a closure slot (`fn(T) -> U`).
///
/// `parse_type_str` (infer.rs) has no type-variable arm, so every one of these
/// lands as `Deferred`; the name is the only thing that tells them apart.
fn is_builtin_type_param_name(n: &str) -> bool {
    !n.starts_with("fn(")
        && !DEFERRED_PREFIXES.iter().any(|p| n.starts_with(p))
        && !n.is_empty()
        && n.len() <= 2
        && n.starts_with(|c: char| c.is_ascii_uppercase())
        && n.chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Structurally match a builtin's declared parameter type against a concrete
/// argument type, recording every `T ↦ concrete` binding it implies.
///
/// Used to keep a *generic* builtin from degrading into an *untyped* one: a
/// signature that names `T` twice (`arr_push([T], T)`, `arr_contains`,
/// `arr_concat`) means the two slots must agree. Unresolved argument types
/// (empty-array literals, inference variables) bind nothing — silence, not a
/// guess.
fn collect_builtin_type_param_bindings(param: &Type, arg: &Type, out: &mut Vec<(String, Type)>) {
    match (param, arg) {
        (Type::Deferred(n), a) if is_builtin_type_param_name(n) => {
            if !type_contains_unresolved(a) {
                out.push((n.clone(), a.clone()));
            }
        }
        (Type::Slice(p), Type::Slice(a))
        | (Type::Option(p), Type::Option(a))
        | (Type::Chan(p), Type::Chan(a)) => collect_builtin_type_param_bindings(p, a, out),
        (Type::Result(po, pe), Type::Result(ao, ae)) => {
            collect_builtin_type_param_bindings(po, ao, out);
            collect_builtin_type_param_bindings(pe, ae, out);
        }
        (Type::Tuple(ps), Type::Tuple(as_)) if ps.len() == as_.len() => {
            for (p, a) in ps.iter().zip(as_.iter()) {
                collect_builtin_type_param_bindings(p, a, out);
            }
        }
        _ => {}
    }
}

fn is_integer_widening(from: &Type, to: &Type) -> bool {
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
fn is_int_width(t: &Type) -> bool {
    matches!(
        t,
        Type::I8 | Type::I16 | Type::I32 | Type::I64 | Type::U8 | Type::U16 | Type::U32 | Type::U64
    )
}

// ── Known primitives (for R08) ────────────────────────────────────────────────

const PRIMITIVE_NAMES: &[&str] = &[
    "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "f32", "f64", "Decimal", "bool", "str",
    "String", "()", "unit",
];

/// Deferred type name prefixes (R08 / R12): always valid, never emit E0308.
/// `Dict` is here because the string-keyed map primitive doesn't have a
/// first-class generic surface yet — `Dict` annotations type-check as
/// deferred until that lands.
const DEFERRED_PREFIXES: &[&str] = &["Uncertain", "Temporal", "Goal", "Dict"];

/// Phase 5: the value a refinement predicate's binder `_` is bound to when
/// discharging a constant-argument obligation. Only forms this slice can fold
/// are represented (an i64 constant, or a string-literal value — which serves
/// both `str_len(_)` length reasoning and `str_eq(_, "lit")` equality).
/// Phase 13 Slice 2: Float added for distribution moment predicates.
#[derive(Clone)]
#[allow(dead_code)]
enum RefineVal {
    Int(i64),
    Float(f64),
    Str(String),
    /// A struct value: field name → its (constant) RefineVal. Backs whole-struct
    /// refinements where the binder `_` is the instance and `_.field` projects.
    Struct(HashMap<String, RefineVal>),
}

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
    /// Phase 5: the refinement name of the CURRENT function's return type, if it
    /// is a named refinement (`-> Positive`). Drives the return-site obligation
    /// (R03): a constant return value is checked against the predicate.
    current_ret_refinement: Option<String>,
    /// Exact node-path of the CURRENT function body (`#fn_<name>.body`). The
    /// R07 body-tail return-type check (E0307) must fire ONLY for this path —
    /// `ends_with(".body")` alone also matches match-arm bodies
    /// (`…arm_N.body`), which would compare an arm's tail against the function's
    /// return type and false-flag (e.g. `state = match s { _ => { … } }`).
    fn_body_path: String,
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
    /// Method table: concrete type name → set of method names defined on it
    /// (across all impl blocks, trait and inherent). Built during
    /// `check_program`; used to tell a genuine method call `p.m()` from calling
    /// a DATA field `p.x()` (E0403).
    type_methods: HashMap<String, HashSet<String>>,
    /// Per-function generic bounds: fn_name → Vec<(param_name, trait_names)>.
    /// Built from `FnDef.generic_bounds` during `check_program` for E0504.
    fn_bounds: HashMap<String, Vec<(String, Vec<String>)>>,
    /// Span of the statement (or sub-expression) currently being checked.
    /// Updated at every `Stmt` boundary inside `check_expr` so that diagnostics
    /// emitted from deeper visits carry source-location info even when the
    /// individual `Expr` variant has no span field of its own.
    current_span: crate::span::Span,
    /// Layer-1 ASI: identifiers in the current fn body whose `.confidence`
    /// field is accessed somewhere. Populated on entry to `check_fn` via a
    /// pre-walk, then read in `Expr::FieldAccess` to suppress W0701.
    confidence_observed: HashSet<String>,
    /// `@[adaptive]` fn names (populated in `check_program`).
    adaptive_fns: HashSet<String>,
    /// `@[pure]` fn names (Phase 5 §2). A `@[pure]` fn may only call other
    /// `@[pure]` fns and the pure-builtin allowlist; `check_purity` enforces it.
    pure_fns: HashSet<String>,
    /// `@[total]` fn names (Phase 5 §3). A `@[total]` fn may only call other
    /// `@[total]` fns and (always-terminating) builtins — otherwise it could
    /// launder non-termination through an un-annotated helper; `check_totality`
    /// enforces it (E1208).
    total_fns: HashSet<String>,
    /// `@[total]` fn → body, for mutual-recursion cycle detection (a 2+-fn cycle
    /// among total fns has no per-fn decreasing measure, so it can't be proven to
    /// terminate → E1208, sound by refusal).
    total_fn_defs: HashMap<String, Expr>,
    /// Names of impl methods whose body is IMPURE (calls an impure builtin / a
    /// non-`@[pure]` fn / spawn / …). A `@[pure]` fn calling such a method via
    /// `x.m()` is E1207 — closes the MethodCall hole in the purity gate (a pure
    /// getter, whose body is pure, is NOT here, so it stays callable: no false
    /// positive). Over dispatch: a name is impure if ANY impl method of that name
    /// is impure (the checker has no receiver type — conservative, sound).
    impure_method_names: HashSet<String>,
    /// Phase 5: `@[pure]` fn → (param names, body expr). Lets a refinement
    /// predicate CALL a pure function over the bound constant (depth ≤ 4),
    /// inlining its body in the constant evaluator. Pure fns are I/O-free
    /// (enforced by check_purity), so this is a safe compile-time evaluation.
    pure_fn_defs: HashMap<String, (Vec<String>, Expr)>,
    /// Phase 5: named refinement types (`type Positive = i64 where …`) → their
    /// erased base `Type`. Recognised as valid type annotations (no E0308) and
    /// resolved to the base wherever a value's type is computed (so `n: Positive`
    /// is treated as `i64` for arithmetic / arg compatibility / etc.).
    refinement_base: HashMap<String, Type>,
    /// Phase 5: named refinement → its predicate Expr (the binder is `_`). Used
    /// to discharge the constant-argument proof obligation (sub-slice 3): at a
    /// call `f(arg)` whose param is a refinement, if `arg` is a compile-time
    /// constant, the predicate is evaluated with `_` bound to it (E1201/E1202).
    refinement_pred: HashMap<String, Expr>,
    /// Phase 5: per-user-fn, the refinement name of each parameter slot (None for
    /// an unrefined param). Populated from fn signatures so a call site can find
    /// which arguments carry a proof obligation.
    fn_param_refinements: HashMap<String, Vec<Option<String>>>,
    /// Phase 5: per-struct, `field_name → refinement_name` for fields whose
    /// declared type is a refinement. Drives the R04 struct-construction
    /// obligation (a constant field value is checked against the predicate).
    struct_field_refinements: HashMap<String, HashMap<String, String>>,
    /// Phase 5: a WHOLE-struct refinement predicate per struct (`type Range = {…}
    /// where _.lo <= _.hi`). Checked at construction when all fields are constant.
    struct_refinements: HashMap<String, Expr>,
    /// `@[sensitive(...)]` struct type names → the category (pii/phi/financial/…).
    /// Such a value may not flow into an external AI call (E1206, PRD §4).
    sensitive_types: HashMap<String, String>,
    /// Transitive taint (PRD §4 / R6): user-fn name → the set of its parameter
    /// INDICES whose value reaches an exfiltration sink (an AI call / write_file
    /// / exec — directly, or through another exfiltrating fn). Computed to a
    /// fixpoint in `check_program`. A sensitive value passed at one of these
    /// argument positions is E1206, closing the "launder through a helper" hole.
    exfiltrating_params: HashMap<String, HashSet<usize>>,
    /// Return-value taint (PRD §4 / R6): user-fn name → the set of its parameter
    /// INDICES whose sensitivity flows to the RETURN value (the fn returns the
    /// param, a field of it, or the result of calling another taint-returning fn
    /// with it). Computed in the same fixpoint. A call `f(sensitive_arg)` at such
    /// a position yields a sensitive result, so `let e = get_email(u)` taints `e`.
    taint_returning_params: HashMap<String, HashSet<usize>>,
    /// Local taint (PRD §4 / R6): within the current function, local-binding name
    /// → (source description, category) for a local that was bound to a sensitive
    /// value (a sensitive struct, or a field of one — `let e = u.email`). Lets
    /// `sink(e)` be flagged even though `e`'s static type is a plain `str`. Reset
    /// per function in `check_fn`; populated as `let` bindings are walked.
    sensitive_locals: HashMap<String, (String, String)>,
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
            current_ret_refinement: Option::None,
            fn_body_path: String::new(),
            known_enums: Vec::new(),
            enum_variants: HashMap::new(),
            current_generic_params: HashSet::new(),
            trait_defs: HashMap::new(),
            impl_table: HashMap::new(),
            type_methods: HashMap::new(),
            fn_bounds: HashMap::new(),
            current_span: crate::span::Span::dummy(),
            confidence_observed: HashSet::new(),
            adaptive_fns: HashSet::new(),
            pure_fns: HashSet::new(),
            total_fns: HashSet::new(),
            total_fn_defs: HashMap::new(),
            impure_method_names: HashSet::new(),
            pure_fn_defs: HashMap::new(),
            refinement_base: HashMap::new(),
            refinement_pred: HashMap::new(),
            fn_param_refinements: HashMap::new(),
            struct_field_refinements: HashMap::new(),
            struct_refinements: HashMap::new(),
            sensitive_types: HashMap::new(),
            exfiltrating_params: HashMap::new(),
            taint_returning_params: HashMap::new(),
            sensitive_locals: HashMap::new(),
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
                Item::ImplBlock(blk) => {
                    let ty_name = axon_type_name(&blk.for_type);
                    // Record: type implements trait_name (E0504), for trait impls.
                    if !blk.trait_name.is_empty() {
                        self.impl_table
                            .entry(ty_name.clone())
                            .or_default()
                            .insert(blk.trait_name.clone());
                    }
                    // Record every method name defined on this type (trait AND
                    // inherent impls) so `p.method()` can be told from calling a
                    // data field `p.x()` (E0403).
                    let methods = self.type_methods.entry(ty_name).or_default();
                    for m in &blk.methods {
                        methods.insert(m.name.clone());
                    }
                }
                Item::FnDef(f) if !f.generic_bounds.is_empty() => {
                    self.fn_bounds
                        .insert(f.name.clone(), f.generic_bounds.clone());
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

        // Collect @[adaptive] fn names for E1500 validation.
        for item in &program.items {
            if let Item::FnDef(f) = item {
                if f.attrs.iter().any(|a| a.name == "adaptive") {
                    self.adaptive_fns.insert(f.name.clone());
                }
            }
        }

        // Phase 5 §2: collect @[pure] fn names, then enforce purity (P01/P02/P04).
        // A @[pure] fn may only call other @[pure] fns + the pure-builtin
        // allowlist; an impure call is E1207. Done as a pre-pass so a @[pure] fn
        // calling another (forward-declared) @[pure] fn is accepted (P05).
        for item in &program.items {
            if let Item::FnDef(f) = item {
                if f.attrs.iter().any(|a| a.name == "pure") {
                    self.pure_fns.insert(f.name.clone());
                    // Record (params, body) for inlining into a predicate.
                    let pnames: Vec<String> = f.params.iter().map(|p| p.name.clone()).collect();
                    self.pure_fn_defs
                        .insert(f.name.clone(), (pnames, f.body.clone()));
                }
                // Phase 5 §3: collect @[total] fn names + bodies (pre-pass, so a
                // total fn calling a forward-declared total fn is accepted, and
                // the body map supports mutual-recursion cycle detection).
                if f.attrs.iter().any(|a| a.name == "total") {
                    self.total_fns.insert(f.name.clone());
                    self.total_fn_defs.insert(f.name.clone(), f.body.clone());
                }
            }
            // Phase 5: register named refinements → erased base + predicate.
            if let Item::RefineDef(r) = item {
                self.refinement_base
                    .insert(r.name.clone(), axon_type_to_type(&r.base));
                self.refinement_pred
                    .insert(r.name.clone(), (*r.predicate).clone());
                // A refinement predicate is a STATIC, deterministic contract: it
                // must be pure. An impure builtin in it (now_ms/random_i64/IO/AI…)
                // makes the "type" non-deterministic and meaningless. Reject it.
                self.reject_impure_refinement(&r.predicate, &r.name);
            }
            // Whole-struct refinement predicates must be pure for the same reason.
            if let Item::TypeDef(td) = item {
                if let Some(pred) = &td.refinement {
                    self.reject_impure_refinement(pred, &td.name);
                }
            }
        }
        // Phase 5: record each user fn's per-param refinement name (None if the
        // param isn't a refinement) so a call site can find proof obligations.
        // Enter this pass if there's ANY refinement — named, or a whole-struct
        // predicate (which needs no named refinement).
        let has_struct_refine = program
            .items
            .iter()
            .any(|i| matches!(i, Item::TypeDef(td) if td.refinement.is_some()));
        if !self.refinement_pred.is_empty() || has_struct_refine {
            for item in &program.items {
                if let Item::FnDef(f) = item {
                    let slots: Vec<Option<String>> = f
                        .params
                        .iter()
                        .map(|p| match &p.ty {
                            AxonType::Named(n) if self.refinement_pred.contains_key(n) => {
                                Some(n.clone())
                            }
                            _ => None,
                        })
                        .collect();
                    if slots.iter().any(|s| s.is_some()) {
                        self.fn_param_refinements.insert(f.name.clone(), slots);
                    }
                }
                // Struct fields whose declared type is a refinement (R04).
                if let Item::TypeDef(td) = item {
                    let refs: HashMap<String, String> = td
                        .fields
                        .iter()
                        .filter_map(|f| match &f.ty {
                            AxonType::Named(n) if self.refinement_pred.contains_key(n) => {
                                Some((f.name.clone(), n.clone()))
                            }
                            _ => None,
                        })
                        .collect();
                    if !refs.is_empty() {
                        self.struct_field_refinements.insert(td.name.clone(), refs);
                    }
                    // Whole-struct refinement predicate (`{…} where _.lo <= _.hi`).
                    if let Some(pred) = &td.refinement {
                        self.struct_refinements
                            .insert(td.name.clone(), (**pred).clone());
                    }
                }
            }
        }
        // Determine which impl-method NAMES are impure (body calls an impure
        // builtin / a non-@[pure] fn / spawn / …), so a @[pure] fn calling such a
        // method via `x.m()` can be flagged. A pure getter has no violations →
        // stays callable. Computed once, after pure_fns is populated.
        for item in &program.items {
            if let Item::ImplBlock(blk) = item {
                for m in &blk.methods {
                    let mut v: Vec<(String, &'static str)> = Vec::new();
                    Self::collect_purity_violations(&m.body, &self.pure_fns, &mut v);
                    if !v.is_empty() {
                        self.impure_method_names.insert(m.name.clone());
                    }
                }
            }
        }
        if !self.pure_fns.is_empty() {
            for item in &program.items {
                if let Item::FnDef(f) = item {
                    if self.pure_fns.contains(&f.name) {
                        self.check_purity(f);
                    }
                }
            }
        }
        // …and @[pure] IMPL METHODS — `check_program`'s loops only matched
        // `Item::FnDef`, so a `@[pure]`/`@[total]` attribute on an impl-block
        // method was silently UNENFORCED (the capability-surface item-walk gap).
        // Check each method's own body directly (run unconditionally — a program
        // may have pure methods but no free pure fns, so the guard above wouldn't
        // fire). Methods are NOT added to `pure_fns`: they dispatch via MethodCall
        // not Ident, and a bare method name could collide with a free fn — so
        // method-to-METHOD purity stays a documented residual; the method's own
        // body (I/O, impure free-fn calls) is now enforced.
        for item in &program.items {
            if let Item::ImplBlock(blk) = item {
                for m in &blk.methods {
                    if m.attrs.iter().any(|a| a.name == "pure") {
                        self.check_purity(m);
                    }
                }
            }
        }

        // R23 eBPF: a `@[bpf]` program is deterministic-by-construction. Validate
        // its kind (E2302) and its helper calls against the capability allowlist
        // (E2300) here; the auto-implied @[total]/@[no_alloc] gates below pick up
        // @[bpf] fns via `fn_is_total`/`fn_is_no_alloc`.
        self.check_bpf_programs(program);

        // SQLi-by-construction: every `sql_query(template, params)` must have a
        // string-LITERAL template, so user data can only enter as a bound `?`
        // parameter — a concatenated/interpolated template is E1210. Walks free
        // fns and impl methods (an unsafe query laundered through a method body
        // is caught too). Not gated on @[contained] — injection is checked always.
        for item in &program.items {
            match item {
                Item::FnDef(f) => self.check_sql_safety(&f.body, f.span),
                Item::ImplBlock(blk) => {
                    for m in &blk.methods {
                        self.check_sql_safety(&m.body, m.span);
                    }
                }
                _ => {}
            }
        }

        // Phase 5 §3: a @[total] fn must terminate. For a recursive @[total] fn,
        // require a strictly-decreasing well-founded measure at every recursive
        // call; a non-recursive @[total] fn passes silently. E1208 otherwise.
        // R23: `@[bpf]` implies `@[total]`.
        for item in &program.items {
            if let Item::FnDef(f) = item {
                if Self::fn_is_total(f) {
                    self.check_totality(f);
                }
            }
            // @[total] IMPL METHODS too (same item-walk gap as @[pure] above): a
            // `@[total]` method with an unbounded `while` / non-decreasing
            // recursion was silently accepted. check_totality enforces the
            // method's own body; method-to-method totality is the same documented
            // residual as @[pure] (methods aren't added to total_fns).
            if let Item::ImplBlock(blk) = item {
                for m in &blk.methods {
                    if m.attrs.iter().any(|a| a.name == "total") {
                        self.check_totality(m);
                    }
                }
            }
        }

        // R17 Slice 3 (§4 / E1704): a `@[no_alloc]` fn (ISR / early-boot) must
        // not reach any heap-allocating operation — directly OR transitively.
        // Compute the set of user fns that (transitively) allocate, to a
        // fixpoint, then flag each `@[no_alloc]` fn that allocates. The
        // transitive analysis closes the laundering hole: hiding a `str_concat`
        // behind an un-annotated helper still trips E1704.
        // R23: `@[bpf]` implies `@[no_alloc]`.
        let has_no_alloc = program.items.iter().any(|it| match it {
            Item::FnDef(f) => Self::fn_is_no_alloc(f),
            Item::ImplBlock(blk) => blk
                .methods
                .iter()
                .any(|m| m.attrs.iter().any(|a| a.name == "no_alloc")),
            _ => false,
        });
        if has_no_alloc {
            let allocating = Self::compute_allocating_fns(program);
            for item in &program.items {
                if let Item::FnDef(f) = item {
                    if Self::fn_is_no_alloc(f) {
                        self.check_no_alloc(f, &allocating);
                    }
                }
                if let Item::ImplBlock(blk) = item {
                    for m in &blk.methods {
                        if m.attrs.iter().any(|a| a.name == "no_alloc") {
                            self.check_no_alloc(m, &allocating);
                        }
                    }
                }
            }
        }

        // R24 TEE (§2 / E1810): `tee_unseal` — declassifying a sealed Secret —
        // may ONLY be called from inside an `@[enclave]`-annotated fn. Scan every
        // NON-enclave fn (and impl method) body for a `tee_unseal` call and emit
        // E1810 for each. The rule is lexical-by-design (no laundering hole: a
        // helper that calls `tee_unseal` must itself carry `@[enclave]`, or it
        // trips E1810 — there is no un-annotated fn that may unseal). This is a
        // pure type/checker rule, enforced with NO TEE hardware; it is the real,
        // locally-verifiable differentiator of the confidential-computing target.
        for item in &program.items {
            match item {
                Item::FnDef(f) if !f.attrs.iter().any(|a| a.name == "enclave") => {
                    self.check_enclave_unseal(&f.name, &f.body, f.span);
                }
                Item::ImplBlock(blk) => {
                    for m in &blk.methods {
                        if !m.attrs.iter().any(|a| a.name == "enclave") {
                            self.check_enclave_unseal(&m.name, &m.body, m.span);
                        }
                    }
                }
                _ => {}
            }
        }

        // PRD §4: collect `@[sensitive(category)]` struct types. A value of such
        // a type may not flow into an external AI call (E1206). The category
        // (pii/phi/financial/…) is the attr's first arg, "sensitive" if absent.
        for item in &program.items {
            if let Item::TypeDef(t) = item {
                if let Some(a) = t.attrs.iter().find(|a| a.name == "sensitive") {
                    let category = a
                        .args
                        .first()
                        .cloned()
                        .unwrap_or_else(|| "sensitive".into());
                    self.sensitive_types.insert(t.name.clone(), category);
                }
            }
        }

        // PRD §4 / R6 transitive taint: compute which user-fn parameters reach an
        // exfiltration sink, to a fixpoint, so a sensitive value laundered through
        // a helper (`relay(u.email)` where `relay(s) { ai_complete(s) }`) is still
        // E1206. Only worth computing when something is actually sensitive.
        if !self.sensitive_types.is_empty() {
            self.compute_exfiltrating_params(program);
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
            // Phase 5: a named refinement `type Name = T where P`. Sub-slice 1
            // registers the form; predicate well-formedness + proof obligations
            // (R01-R05, constant-arg comptime / SMT) are later sub-slices.
            Item::RefineDef(_) => {}
            Item::EnumDef(_)
            | Item::ModDecl(_)
            | Item::UseDecl(_)
            | Item::TraitDef(_)
            | Item::LetDef { .. } => {}
        }
    }

    fn check_fn(&mut self, f: &FnDef) {
        // Reset the per-function sensitive-local tracking (R6 local taint).
        // Seed it with any parameter whose declared type is a `@[sensitive]`
        // struct, so `fn leak(u: User) { sink(u.email) }` is covered and a local
        // copy of a param field inherits taint.
        self.sensitive_locals.clear();
        for p in &f.params {
            if let AxonType::Named(tn) = &p.ty {
                if let Some(cat) = self.sensitive_types.get(tn) {
                    self.sensitive_locals
                        .insert(p.name.clone(), (tn.clone(), cat.clone()));
                }
            }
        }
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

        // R5 goal sugar: validate `#[goal(...)]` attributes on the function.
        if let Some(goal_attr) = f.attrs.iter().find(|a| a.name == "goal") {
            // E1504: a `#[goal]` fn must have zero params (entry point, not consumer).
            if !f.params.is_empty() {
                self.errors.push(
                    CheckError::new(
                        E1504,
                        format!(
                            "`{}` is a `#[goal]` function — must have zero params (params are reserved for future use)",
                            f.name
                        ),
                    )
                    .with_span(f.span),
                );
            }
            // E1500: the metric must name an `@[adaptive]` fn.
            let mut metric_name: Option<String> = None;
            let mut all_numbers = true;
            let mut bad_strategy: Option<String> = None;
            for arg in &goal_attr.args {
                if let Some((k, v)) = arg.split_once(':') {
                    let k = k.trim().to_lowercase();
                    let v = v.trim();
                    match k.as_str() {
                        "metric" => metric_name = Some(v.to_string()),
                        "target" | "max_evals" | "holdout" | "lo" | "hi"
                            if v.parse::<f64>().is_err() && v.parse::<i64>().is_err() =>
                        {
                            all_numbers = false;
                        }
                        // R5: `test_set: [a, b, c]` (rendered `"a,b,c"`) — every
                        // element must parse as an integer.
                        "test_set" if v.split(',').any(|p| p.trim().parse::<i64>().is_err()) => {
                            all_numbers = false;
                        }
                        // R5 (PRD L889-899): `strategy:` must be one of the
                        // closed set; an unknown name is E1505.
                        "strategy" => {
                            let known = matches!(
                                v.to_lowercase().as_str(),
                                "hill_climb"
                                    | "hillclimb"
                                    | "random"
                                    | "multistart"
                                    | "tournament"
                                    | "bayesian"
                            );
                            if !known {
                                bad_strategy = Some(v.to_string());
                            }
                        }
                        _ => {}
                    }
                }
            }
            if let Some(ref m) = metric_name {
                let is_adaptive = self.adaptive_fns.contains(m.as_str());
                if !is_adaptive {
                    self.errors.push(
                        CheckError::new(
                            E1500,
                            format!(
                                "`#[goal(metric: {})]` — metric fn `{}` must be annotated `@[adaptive]`",
                                m, m
                            ),
                        )
                        .with_span(f.span),
                    );
                }
            }
            // E1503: numeric fields must parse.
            if !all_numbers {
                self.errors.push(
                    CheckError::new(
                        E1503,
                        "`#[goal(...)]` — target/max_evals/holdout/lo/hi must be numeric values, and test_set must be a list of integers"
                            .to_string(),
                    )
                    .with_span(f.span),
                );
            }
            // E1505: the strategy must be one of the closed set.
            if let Some(bad) = bad_strategy {
                self.errors.push(
                    CheckError::new(
                        E1505,
                        format!(
                            "`@[goal(strategy: {bad})]` — unknown strategy; expected one of \
                             hill_climb | random | multistart | tournament | bayesian"
                        ),
                    )
                    .with_span(f.span),
                );
            }
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

        // Resolve the declared return type for R03 / R07 checks. `enumify` maps
        // known enum names to `Type::Enum` — `axon_type_to_type` is context-free
        // and defaults unknown names to `Struct`, which would otherwise not match
        // an enum-variant literal's inferred `Type::Enum` (e.g. a function that
        // returns an enum: `fn make() -> Plan { Plan::Step { … } }`).
        let known_enums = self.known_enums.clone();
        let resolved_ret = f
            .return_type
            .as_ref()
            .map(|t| self.resolve_refinements(enumify(axon_type_to_type(t), &known_enums)))
            .unwrap_or(Type::Unit);

        let prev_ret = self.current_ret_ty.replace(resolved_ret);
        // Phase 5: track a named-refinement return type for the R03 obligation.
        let ret_refinement = match &f.return_type {
            Option::Some(AxonType::Named(n)) if self.refinement_pred.contains_key(n) => {
                Option::Some(n.clone())
            }
            _ => Option::None,
        };
        let prev_ret_refine = std::mem::replace(&mut self.current_ret_refinement, ret_refinement);

        let fn_path = format!("#fn_{}", f.name);
        let mut scope: HashMap<String, Type> = HashMap::new();

        // Seed scope with parameters (same enum-aware resolution as the return
        // type). Phase 5: a refinement-typed param erases to its base, so
        // `n: Positive` binds as `i64` for arithmetic / arg compatibility.
        for param in &f.params {
            let pty = self.resolve_refinements(enumify(axon_type_to_type(&param.ty), &known_enums));
            scope.insert(param.name.clone(), pty);
        }

        // R5: #[goal] fns bind `goal_met` in their body scope.
        if f.attrs.iter().any(|a| a.name == "goal") {
            scope.insert("goal_met".to_string(), Type::I64);
        }

        // Layer-1 ASI: pre-walk the body to collect identifier names whose
        // `.confidence` field is accessed somewhere. This drives the W0701
        // heuristic raised in `Expr::FieldAccess` for `.value` accesses.
        let prev_observed = std::mem::take(&mut self.confidence_observed);
        Self::collect_confidence_observed(&f.body, &mut self.confidence_observed);

        let body_path = format!("{fn_path}.body");
        let prev_body_path = std::mem::replace(&mut self.fn_body_path, body_path.clone());
        self.check_expr(&f.body, &body_path, &mut scope);
        self.fn_body_path = prev_body_path;

        self.confidence_observed = prev_observed;
        self.current_ret_ty = prev_ret;
        self.current_ret_refinement = prev_ret_refine;
        self.current_generic_params = prev_generics;
        self.current_span = prev_span;
    }

    /// Phase 5 §1 sub-slice 3 — the constant-argument proof obligation (R02).
    /// At a call `f(arg₁, …)` whose parameter `i` is a refinement `T where P`,
    /// if `argᵢ` is a compile-time constant, evaluate `P[argᵢ/_]`: a `false`
    /// result is E1201 (the refinement is provably violated). A non-constant
    /// argument can't be discharged here — that needs the SMT backend (§4) — so
    /// it's silently left for the runtime/solver (no E1202 noise yet). No Z3.
    fn check_refinement_args(&mut self, fn_name: &str, args: &[Expr], node_path: &str) {
        let Some(slots) = self.fn_param_refinements.get(fn_name).cloned() else {
            return;
        };
        for (i, arg) in args.iter().enumerate() {
            let Some(Some(rname)) = slots.get(i) else {
                continue;
            };
            let Some(pred) = self.refinement_pred.get(rname).cloned() else {
                continue;
            };
            // The binder value: an i64 constant, or a string literal's length
            // (for `str_len(_)` predicates). Anything else is non-constant and
            // deferred (no obligation discharged here).
            let bound = if let Some(v) = const_eval_int(arg) {
                RefineVal::Int(v)
            } else if let Expr::Literal(crate::ast::Literal::Str(s)) = arg {
                RefineVal::Str(s.clone())
            } else {
                continue;
            };
            // Only a PROVABLY-false predicate is an error; satisfied or
            // not-statically-evaluable both defer (the latter to §4 / runtime).
            if self.eval_refinement_pred(&pred, &bound) == Some(false) {
                let file = self.file.clone();
                let span = self.current_span;
                let shown = match bound {
                    RefineVal::Int(v) => v.to_string(),
                    RefineVal::Float(f) => f.to_string(),
                    RefineVal::Str(s) => format!("{s:?}"),
                    RefineVal::Struct(_) => "the struct value".to_string(),
                };
                self.errors.push(
                    CheckError::new(
                        E1209,
                        format!(
                            "argument {i} of `{fn_name}` ({shown}) violates the refinement \
                             `{rname}` — the constant does not satisfy the type's predicate"
                        ),
                    )
                    .node(format!("{node_path}.arg_{i}"))
                    .at(&file, 0, 0)
                    .with_span(span)
                    .fix(format!(
                        "pass a value that satisfies `{rname}`'s predicate, or change the \
                         parameter type"
                    )),
                );
            }
        }
    }

    /// Phase 5 R03 — the return-site obligation. When the current function's
    /// return type is a named refinement and `ret_expr` is a compile-time
    /// constant, the predicate must hold for it (else E1209). Same soundness as
    /// the argument obligation: only a provably-false constant errors.
    fn check_return_refinement(&mut self, ret_expr: &Expr, node_path: &str) {
        let Some(rname) = self.current_ret_refinement.clone() else {
            return;
        };
        let Some(pred) = self.refinement_pred.get(&rname).cloned() else {
            return;
        };
        let bound = if let Some(v) = const_eval_int(ret_expr) {
            RefineVal::Int(v)
        } else if let Expr::Literal(crate::ast::Literal::Str(s)) = ret_expr {
            RefineVal::Str(s.clone())
        } else {
            return;
        };
        if self.eval_refinement_pred(&pred, &bound) == Some(false) {
            let file = self.file.clone();
            let span = self.current_span;
            let shown = match bound {
                RefineVal::Int(v) => v.to_string(),
                RefineVal::Float(f) => f.to_string(),
                RefineVal::Str(s) => format!("{s:?}"),
                RefineVal::Struct(_) => "the struct value".to_string(),
            };
            self.errors.push(
                CheckError::new(
                    E1209,
                    format!(
                        "the returned constant ({shown}) violates the refinement return type \
                         `{rname}` — it does not satisfy the type's predicate"
                    ),
                )
                .node(node_path)
                .at(&file, 0, 0)
                .with_span(span)
                .fix(format!(
                    "return a value that satisfies `{rname}`'s predicate, or change the return type"
                )),
            );
        }
    }

    /// Phase 5 — the let-binding refinement obligation. A `let p: T = <value>`
    /// whose annotation `T` is a refinement, with a CONSTANT value, must satisfy
    /// the predicate (else E1209). Mirrors `check_return_refinement`; a
    /// non-constant value defers to the runtime check. Only a provably-false
    /// constant errors (sound). No-op when the annotation isn't a refinement.
    fn check_let_refinement(
        &mut self,
        annot: &Option<AxonType>,
        name: &str,
        value: &Expr,
        node_path: &str,
    ) {
        let Some(AxonType::Named(rname)) = annot else {
            return;
        };
        let Some(pred) = self.refinement_pred.get(rname).cloned() else {
            return;
        };
        let bound = if let Some(v) = const_eval_int(value) {
            RefineVal::Int(v)
        } else if let Expr::Literal(crate::ast::Literal::Str(s)) = value {
            RefineVal::Str(s.clone())
        } else {
            return;
        };
        if self.eval_refinement_pred(&pred, &bound) == Some(false) {
            let file = self.file.clone();
            let span = self.current_span;
            let shown = match bound {
                RefineVal::Int(v) => v.to_string(),
                RefineVal::Float(f) => f.to_string(),
                RefineVal::Str(s) => format!("{s:?}"),
                RefineVal::Struct(_) => "the struct value".to_string(),
            };
            self.errors.push(
                CheckError::new(
                    E1209,
                    format!(
                        "the value bound to `{name}` ({shown}) violates the refinement `{rname}` \
                         — it does not satisfy the type's predicate"
                    ),
                )
                .node(node_path)
                .at(&file, 0, 0)
                .with_span(span)
                .fix(format!(
                    "bind a value that satisfies `{rname}`'s predicate, or change the annotation"
                )),
            );
        }
    }

    /// Phase 5 R04 — the struct-construction obligation. A constant value
    /// assigned to a refinement-typed field must satisfy the predicate (E1209).
    fn check_field_refinement(
        &mut self,
        rname: &str,
        fname: &str,
        struct_name: &str,
        fexpr: &Expr,
        node_path: &str,
    ) {
        let Some(pred) = self.refinement_pred.get(rname).cloned() else {
            return;
        };
        let bound = if let Some(v) = const_eval_int(fexpr) {
            RefineVal::Int(v)
        } else if let Expr::Literal(crate::ast::Literal::Str(s)) = fexpr {
            RefineVal::Str(s.clone())
        } else {
            return;
        };
        if self.eval_refinement_pred(&pred, &bound) == Some(false) {
            let file = self.file.clone();
            let span = self.current_span;
            let shown = match bound {
                RefineVal::Int(v) => v.to_string(),
                RefineVal::Float(f) => f.to_string(),
                RefineVal::Str(s) => format!("{s:?}"),
                RefineVal::Struct(_) => "the struct value".to_string(),
            };
            self.errors.push(
                CheckError::new(
                    E1209,
                    format!(
                        "field `{fname}` of `{struct_name}` is set to {shown}, which violates its \
                         refinement type `{rname}` — the constant does not satisfy the predicate"
                    ),
                )
                .node(node_path)
                .at(&file, 0, 0)
                .with_span(span)
                .fix(format!(
                    "set `{fname}` to a value that satisfies `{rname}`'s predicate"
                )),
            );
        }
    }

    /// Evaluate a refinement predicate with the binder `_` bound to a constant
    /// value. Supports the Phase-5 predicate subset over constants: integer
    /// arithmetic, comparisons, `&&`/`||`/`!`, `str_len`/`str_eq`, and CALLS into
    /// `@[pure]` functions (depth ≤ 4 — the body is inlined with its parameters
    /// bound to the evaluated arguments). Returns `None` when the predicate uses
    /// a form this evaluator can't fold (the obligation is then deferred).
    fn eval_refinement_pred(&self, pred: &Expr, bound: &RefineVal) -> Option<bool> {
        let mut env = HashMap::new();
        env.insert("_".to_string(), bound.clone());
        self.eval_pred_bool(pred, &env, 0)
    }

    fn eval_pred_bool(
        &self,
        e: &Expr,
        env: &HashMap<String, RefineVal>,
        depth: u32,
    ) -> Option<bool> {
        use crate::ast::BinOp;
        match e {
            Expr::Literal(crate::ast::Literal::Bool(v)) => Some(*v),
            Expr::UnaryOp {
                op: crate::ast::UnaryOp::Not,
                operand,
            } => Some(!self.eval_pred_bool(operand, env, depth)?),
            Expr::Call { callee, args, .. } => {
                if let Expr::Ident(fname) = callee.as_ref() {
                    // `str_eq(x, "lit")` — equality of a bound string vs a literal.
                    if fname == "str_eq" && args.len() == 2 {
                        let l = self.eval_pred_str(&args[0], env);
                        let r = self.eval_pred_str(&args[1], env);
                        if let (Some(a), Some(b)) = (l, r) {
                            return Some(a == b);
                        }
                        return None;
                    }
                    // A call into a @[pure] fn returning bool — inline its body
                    // (depth ≤ 4), binding params to the evaluated args.
                    if let Some(v) = self.eval_pure_call_bool(fname, args, env, depth) {
                        return Some(v);
                    }
                }
                None
            }
            Expr::BinOp { op, left, right } => match op {
                BinOp::And => Some(
                    self.eval_pred_bool(left, env, depth)?
                        && self.eval_pred_bool(right, env, depth)?,
                ),
                BinOp::Or => Some(
                    self.eval_pred_bool(left, env, depth)?
                        || self.eval_pred_bool(right, env, depth)?,
                ),
                BinOp::Eq | BinOp::NotEq | BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq => {
                    // Try integer comparison first; fall back to float (Phase 13
                    // distribution moment predicates produce f64 values).
                    if let (Some(l), Some(r)) = (
                        self.eval_pred_int(left, env, depth),
                        self.eval_pred_int(right, env, depth),
                    ) {
                        return Some(match op {
                            BinOp::Eq => l == r,
                            BinOp::NotEq => l != r,
                            BinOp::Lt => l < r,
                            BinOp::Gt => l > r,
                            BinOp::LtEq => l <= r,
                            BinOp::GtEq => l >= r,
                            _ => unreachable!(),
                        });
                    }
                    if let (Some(l), Some(r)) = (
                        self.eval_pred_f64(left, env, depth),
                        self.eval_pred_f64(right, env, depth),
                    ) {
                        return Some(match op {
                            BinOp::Eq => (l - r).abs() < f64::EPSILON,
                            BinOp::NotEq => (l - r).abs() >= f64::EPSILON,
                            BinOp::Lt => l < r,
                            BinOp::Gt => l > r,
                            BinOp::LtEq => l <= r,
                            BinOp::GtEq => l >= r,
                            _ => unreachable!(),
                        });
                    }
                    None
                }
                _ => None,
            },
            // A pure-fn body may be a single `if c { a } else { b }` returning bool.
            Expr::If { cond, then, else_ } => {
                let c = self.eval_pred_bool(cond, env, depth)?;
                if c {
                    self.eval_pred_bool(then, env, depth)
                } else {
                    self.eval_pred_bool(else_.as_ref()?, env, depth)
                }
            }
            Expr::Block(stmts) => {
                // A pure-fn body block: evaluate to its tail expression.
                self.eval_pred_bool(&stmts.last()?.expr, env, depth)
            }
            // A bare bool identifier isn't representable in RefineVal (int/str
            // only) — defer rather than guess.
            _ => None,
        }
    }

    /// Resolve a predicate sub-expression to a string (a bound str var or a
    /// string literal).
    fn eval_pred_str(&self, e: &Expr, env: &HashMap<String, RefineVal>) -> Option<String> {
        match e {
            Expr::Ident(n) => match env.get(n)? {
                RefineVal::Str(s) => Some(s.clone()),
                _ => None,
            },
            Expr::Literal(crate::ast::Literal::Str(s)) => Some(s.clone()),
            _ => None,
        }
    }

    /// Inline a `@[pure]` function call that returns bool, binding its params to
    /// the evaluated arguments. Depth-bounded (≤ 4) so a recursive/expensive
    /// predicate can't blow the checker; returns `None` past the limit (defer).
    fn eval_pure_call_bool(
        &self,
        fname: &str,
        args: &[Expr],
        env: &HashMap<String, RefineVal>,
        depth: u32,
    ) -> Option<bool> {
        if depth >= 4 {
            return None;
        }
        let (params, body) = self.pure_fn_defs.get(fname)?;
        if params.len() != args.len() {
            return None;
        }
        let mut new_env = HashMap::new();
        for (p, a) in params.iter().zip(args.iter()) {
            // Each arg must evaluate to an int (the only param kind a constant
            // predicate can pass through today) — str-param pure fns defer.
            let v = self.eval_pred_int(a, env, depth)?;
            new_env.insert(p.clone(), RefineVal::Int(v));
        }
        self.eval_pred_bool(body, &new_env, depth + 1)
    }

    /// Evaluate an integer-valued sub-expression of a predicate.
    fn eval_pred_int(&self, e: &Expr, env: &HashMap<String, RefineVal>, depth: u32) -> Option<i64> {
        use crate::ast::{BinOp, Literal, UnaryOp};
        match e {
            // A bound variable (`_` or a pure-fn param) → its integer value.
            Expr::Ident(n) => match env.get(n)? {
                RefineVal::Int(v) => Some(*v),
                _ => None,
            },
            // Field projection `_.field` (or `x.field`) → the struct binder's
            // integer field. Supports a whole-struct refinement `_.lo <= _.hi`.
            Expr::FieldAccess { receiver, field } => {
                if let Expr::Ident(n) = receiver.as_ref() {
                    if let RefineVal::Struct(fields) = env.get(n)? {
                        if let RefineVal::Int(v) = fields.get(field)? {
                            return Some(*v);
                        }
                    }
                }
                None
            }
            // `str_len(x)` → the bound string's char count.
            Expr::Call { callee, args, .. } => {
                if let Expr::Ident(fname) = callee.as_ref() {
                    if fname == "str_len" && args.len() == 1 {
                        if let Some(s) = self.eval_pred_str(&args[0], env) {
                            return Some(s.chars().count() as i64);
                        }
                    }
                    // A @[pure] fn returning i64, inlined.
                    if let Some(v) = self.eval_pure_call_int(fname, args, env, depth) {
                        return Some(v);
                    }
                }
                None
            }
            Expr::Literal(Literal::Int(n)) => Some(*n),
            Expr::UnaryOp {
                op: UnaryOp::Neg,
                operand,
            } => self.eval_pred_int(operand, env, depth)?.checked_neg(),
            Expr::If { cond, then, else_ } => {
                if self.eval_pred_bool(cond, env, depth)? {
                    self.eval_pred_int(then, env, depth)
                } else {
                    self.eval_pred_int(else_.as_ref()?, env, depth)
                }
            }
            Expr::Block(stmts) => self.eval_pred_int(&stmts.last()?.expr, env, depth),
            Expr::BinOp { op, left, right } => {
                let l = self.eval_pred_int(left, env, depth)?;
                let r = self.eval_pred_int(right, env, depth)?;
                match op {
                    BinOp::Add => l.checked_add(r),
                    BinOp::Sub => l.checked_sub(r),
                    BinOp::Mul => l.checked_mul(r),
                    BinOp::Div if r != 0 => l.checked_div(r),
                    BinOp::Rem if r != 0 => l.checked_rem(r),
                    _ => None,
                }
            }
            _ => None,
        }
    }

    /// Inline a `@[pure]` function call that returns i64, binding params to the
    /// evaluated int arguments. Depth-bounded (≤ 4).
    fn eval_pure_call_int(
        &self,
        fname: &str,
        args: &[Expr],
        env: &HashMap<String, RefineVal>,
        depth: u32,
    ) -> Option<i64> {
        if depth >= 4 {
            return None;
        }
        let (params, body) = self.pure_fn_defs.get(fname)?;
        if params.len() != args.len() {
            return None;
        }
        let mut new_env = HashMap::new();
        for (p, a) in params.iter().zip(args.iter()) {
            let v = self.eval_pred_int(a, env, depth)?;
            new_env.insert(p.clone(), RefineVal::Int(v));
        }
        self.eval_pred_int(body, &new_env, depth + 1)
    }

    /// Phase 13 Slice 2: Evaluate a float-valued sub-expression of a predicate.
    /// Recognises float literals, bound Float/Int variables, distribution moment
    /// expressions `E[dist]` / `Var[dist]`, probability expressions `P(dist op k)`,
    /// and float arithmetic — all closed-form for constant distribution parameters.
    fn eval_pred_f64(&self, e: &Expr, env: &HashMap<String, RefineVal>, depth: u32) -> Option<f64> {
        use crate::ast::{BinOp, Literal};
        match e {
            Expr::Literal(Literal::Float(f)) => Some(*f),
            Expr::Literal(Literal::Int(n)) => Some(*n as f64),
            Expr::Ident(n) => match env.get(n)? {
                RefineVal::Float(f) => Some(*f),
                RefineVal::Int(n) => Some(*n as f64),
                _ => None,
            },
            // E[dist] / Var[dist] — index notation for distribution moments
            Expr::Index { receiver, index } => {
                if let Expr::Ident(tag) = receiver.as_ref() {
                    if matches!(tag.as_str(), "E" | "Var") {
                        let dist_rv = self.eval_pred_dist(index, env, depth)?;
                        return checker_dist_moment(tag, &dist_rv);
                    }
                }
                None
            }
            // P(dist op k) — tail probability
            Expr::Call { callee, args, .. } => {
                if let Expr::Ident(name) = callee.as_ref() {
                    if name == "P" && args.len() == 1 {
                        if let Expr::BinOp { op, left, right } = &args[0] {
                            let dist_rv = self.eval_pred_dist(left, env, depth)?;
                            let k = self.eval_pred_f64(right, env, depth)?;
                            let cdf = checker_dist_cdf(&dist_rv, k)?;
                            return Some(match op {
                                BinOp::LtEq | BinOp::Lt => cdf,
                                BinOp::Gt | BinOp::GtEq => 1.0 - cdf,
                                _ => return None,
                            });
                        }
                    }
                }
                None
            }
            Expr::BinOp { op, left, right } => {
                let l = self.eval_pred_f64(left, env, depth)?;
                let r = self.eval_pred_f64(right, env, depth)?;
                match op {
                    BinOp::Add => Some(l + r),
                    BinOp::Sub => Some(l - r),
                    BinOp::Mul => Some(l * r),
                    BinOp::Div if r != 0.0 => Some(l / r),
                    _ => None,
                }
            }
            Expr::UnaryOp {
                op: crate::ast::UnaryOp::Neg,
                operand,
            } => Some(-self.eval_pred_f64(operand, env, depth)?),
            _ => None,
        }
    }

    /// Evaluate a distribution-valued sub-expression in a predicate context.
    /// Returns a struct-like map of field name → RefineVal suitable for
    /// `checker_dist_moment` / `checker_dist_cdf`.
    fn eval_pred_dist(
        &self,
        e: &Expr,
        env: &HashMap<String, RefineVal>,
        depth: u32,
    ) -> Option<HashMap<String, f64>> {
        match e {
            Expr::Ident(n) => {
                if let RefineVal::Struct(fields) = env.get(n)? {
                    let mut out = HashMap::new();
                    for (k, v) in fields {
                        if let Some(f) = refine_val_to_f64(v) {
                            out.insert(k.clone(), f);
                        }
                    }
                    Some(out)
                } else {
                    None
                }
            }
            // Constant struct literal in a predicate (unlikely but correct)
            Expr::StructLit { fields, .. } => {
                let mut out = HashMap::new();
                for (k, v) in fields {
                    if let Some(f) = self.eval_pred_f64(v, env, depth) {
                        out.insert(k.clone(), f);
                    }
                }
                if out.is_empty() {
                    None
                } else {
                    Some(out)
                }
            }
            _ => None,
        }
    }

    /// Phase 5: replace any named-refinement type with its erased base,
    /// recursively (a refinement resolves to `Struct(name)` via
    /// `axon_type_to_type`; swap it for the registered base). Idempotent for
    /// non-refinement types.
    fn resolve_refinements(&self, ty: Type) -> Type {
        match ty {
            Type::Struct(ref n) | Type::Deferred(ref n) => {
                if let Some(base) = self.refinement_base.get(n) {
                    base.clone()
                } else {
                    ty
                }
            }
            Type::Option(i) => Type::Option(Box::new(self.resolve_refinements(*i))),
            Type::Slice(i) => Type::Slice(Box::new(self.resolve_refinements(*i))),
            Type::Result(a, b) => Type::Result(
                Box::new(self.resolve_refinements(*a)),
                Box::new(self.resolve_refinements(*b)),
            ),
            other => other,
        }
    }

    /// Phase 5 §2 — enforce `@[pure]` (P01/P02/P04). Walks the body of a
    /// `@[pure]` function and emits E1207 for any operation that is not
    /// referentially transparent: a call to an impure builtin or to a non-pure
    /// user function (P01/P04), `spawn`/channel ops, or a `?`-less side effect.
    /// `@[pure]` propagates one way (P05): a non-pure fn may call a pure one, but
    /// not the reverse — checked here because `pure_fns` is fully populated.
    fn check_purity(&mut self, f: &FnDef) {
        let span = f.span;
        let fname = f.name.clone();
        // `@[pure]` IS the empty effect row (Phase 5 §2 / Phase 6 E06). Declaring
        // both `@[pure]` AND a non-empty `| {…}` row is a direct contradiction —
        // the attribute promises no effects while the row claims some. Flag it so
        // the two annotations can't silently disagree.
        if let Some(row) = &f.effect_row {
            if !row.effects.is_empty() {
                let effs = row.effects.join(", ");
                let file = self.file.clone();
                self.errors.push(
                    CheckError::new(
                        E1207,
                        format!(
                            "`@[pure]` function `{fname}` also declares the effect row \
                             `| {{{effs}}}` — `@[pure]` means the EMPTY effect row, so it \
                             cannot also claim effects"
                        ),
                    )
                    .at(&file, 0, 0)
                    .with_span(span)
                    .fix(format!(
                        "drop the `@[pure]` attribute (keep `| {{{effs}}}`), or drop the \
                         `| {{{effs}}}` row (keep `@[pure]`) — not both"
                    )),
                );
            }
        }
        let mut violations: Vec<(String, &'static str)> = Vec::new();
        Self::collect_purity_violations(&f.body, &self.pure_fns, &mut violations);
        // Also flag `x.m()` where method `m` is impure — collect_purity_violations
        // only inspects Ident callees, so an impure METHOD call slipped through
        // (the MethodCall-vs-Call gap). A pure getter is not in
        // impure_method_names, so it stays callable (no false positive).
        if !self.impure_method_names.is_empty() {
            Self::collect_impure_method_calls(&f.body, &self.impure_method_names, &mut violations);
        }
        for (callee, kind) in violations {
            let file = self.file.clone();
            self.errors.push(
                CheckError::new(
                    E1207,
                    format!(
                        "`@[pure]` function `{fname}` performs an impure operation: {kind} `{callee}` \
                         — a pure function may only call other `@[pure]` functions and pure builtins"
                    ),
                )
                .at(&file, 0, 0)
                .with_span(span)
                .fix(format!(
                    "remove the `@[pure]` attribute from `{fname}`, or replace `{callee}` with a pure \
                     equivalent (no I/O, AI, channels, randomness, time, or mutation)"
                )),
            );
        }
    }

    /// R17 Slice 3 (E1704): compute the set of user-fn names that (transitively)
    /// allocate on the heap. A fn allocates if it directly calls a
    /// heap-allocating builtin, builds an interpolated string (`FmtStr`), or
    /// calls an already-known-allocating user fn. Iterated to a fixpoint so the
    /// transitive case is closed (an allocator hidden behind an un-annotated
    /// helper still propagates up). Mirrors `transitive_builtin_effects`.
    fn compute_allocating_fns(program: &Program) -> HashSet<String> {
        // Map fn-name → set of called user-fn names + a "directly allocates" bit.
        let mut direct: HashMap<String, (bool, Vec<String>)> = HashMap::new();
        for item in &program.items {
            match item {
                Item::FnDef(f) => {
                    direct.insert(f.name.clone(), Self::scan_allocation(&f.body));
                }
                Item::ImplBlock(blk) => {
                    for m in &blk.methods {
                        // Methods dispatch via MethodCall, not by bare name; we
                        // index them by their own name for the direct-alloc bit.
                        direct.insert(m.name.clone(), Self::scan_allocation(&m.body));
                    }
                }
                _ => {}
            }
        }
        let mut allocating: HashSet<String> = HashSet::new();
        // Seed with fns that directly allocate.
        for (name, (d, _)) in &direct {
            if *d {
                allocating.insert(name.clone());
            }
        }
        // Propagate: a fn calling a known-allocating user fn also allocates.
        loop {
            let mut changed = false;
            for (name, (_, calls)) in &direct {
                if allocating.contains(name) {
                    continue;
                }
                if calls.iter().any(|c| allocating.contains(c)) {
                    allocating.insert(name.clone());
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        allocating
    }

    /// Scan a fn body for direct allocation. Returns `(directly_allocates,
    /// called_user_fn_names)`. "Directly allocates" = calls a heap-allocating
    /// builtin or evaluates an interpolated `FmtStr`. Called user-fn names are
    /// the Ident callees that are NOT builtins (resolved transitively by the
    /// fixpoint in `compute_allocating_fns`).
    fn scan_allocation(body: &Expr) -> (bool, Vec<String>) {
        let mut allocates = false;
        let mut calls: Vec<String> = Vec::new();
        Self::scan_allocation_rec(body, &mut allocates, &mut calls);
        (allocates, calls)
    }

    fn scan_allocation_rec(expr: &Expr, allocates: &mut bool, calls: &mut Vec<String>) {
        match expr {
            // An interpolated string materialises a fresh heap str via concat.
            // A single literal segment (no interpolation) does NOT allocate at
            // runtime — it is a static str — so only flag genuine interpolation
            // (≥1 Expr part).
            Expr::FmtStr { parts } if parts.iter().any(|p| matches!(p, FmtPart::Expr(_))) => {
                *allocates = true;
            }
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name) = callee.as_ref() {
                    if crate::builtins::is_heap_allocating_builtin(name) {
                        *allocates = true;
                    } else if !crate::builtins::is_known_builtin(name) {
                        calls.push(name.clone());
                    }
                }
            }
            _ => {}
        }
        Self::for_each_child(expr, &mut |c| {
            Self::scan_allocation_rec(c, allocates, calls)
        });
    }

    /// R17 Slice 3 (E1704): a `@[no_alloc]` fn must not reach a heap allocation.
    /// Emit E1704 naming the first allocating builtin / interpolation / helper.
    fn check_no_alloc(&mut self, f: &FnDef, allocating_fns: &HashSet<String>) {
        let span = f.span;
        let fname = f.name.clone();
        let mut violations: Vec<(String, &'static str)> = Vec::new();
        Self::collect_no_alloc_violations(&f.body, allocating_fns, &mut violations);
        for (callee, kind) in violations {
            let file = self.file.clone();
            self.errors.push(
                CheckError::new(
                    E1704,
                    format!(
                        "`@[no_alloc]` function `{fname}` reaches a heap allocation: {kind} `{callee}` \
                         — an ISR / early-boot fn must be allocation-free"
                    ),
                )
                .at(&file, 0, 0)
                .with_span(span)
                .fix(format!(
                    "remove `@[no_alloc]` from `{fname}`, or eliminate `{callee}` (avoid str/array/dict \
                     construction and string interpolation; use fixed scalars / volatile MMIO)"
                )),
            );
        }
    }

    /// R23 eBPF: a fn is treated as `@[total]` if it is explicitly annotated
    /// `@[total]` OR it is a `@[bpf]` program (BPF programs are bounded by
    /// construction — the verifier rejects unbounded loops, so Axon enforces
    /// `@[total]` up front to refuse them BEFORE codegen).
    fn fn_is_total(f: &FnDef) -> bool {
        f.attrs.iter().any(|a| a.name == "total" || a.name == "bpf")
    }

    /// R23 eBPF: a fn is treated as `@[no_alloc]` if explicitly annotated OR it
    /// is a `@[bpf]` program (eBPF has no heap, so `@[bpf]` implies `@[no_alloc]`).
    fn fn_is_no_alloc(f: &FnDef) -> bool {
        f.attrs
            .iter()
            .any(|a| a.name == "no_alloc" || a.name == "bpf")
    }

    /// R23 eBPF: validate each `@[bpf(kind: K)]` program — the kind is one of the
    /// supported program types (E2302), and every BPF helper it calls is on the
    /// capability allowlist (E2300). The allowlist check is the novelty: an
    /// un-granted helper is refused at CHECK TIME with a clean Axon error, never
    /// at kernel load time. (`@[total]`/`@[no_alloc]` are auto-implied and
    /// enforced by the existing E1208/E1704 gates via `fn_is_total`/`fn_is_no_alloc`.)
    fn check_bpf_programs(&mut self, program: &Program) {
        for item in &program.items {
            let Item::FnDef(f) = item else { continue };
            let Some(bpf_attr) = f.attrs.iter().find(|a| a.name == "bpf") else {
                continue;
            };
            // Validate the program kind (default socket_filter if unspecified).
            let kind = bpf_attr
                .args
                .iter()
                .find_map(|a| {
                    a.strip_prefix("kind:")
                        .map(|s| s.trim().to_string())
                        .or_else(|| {
                            // bare arg form `@[bpf(xdp)]`
                            if !a.contains(':') {
                                Some(a.trim().to_string())
                            } else {
                                None
                            }
                        })
                })
                .unwrap_or_else(|| "socket_filter".to_string());
            if crate::builtins::bpf_kind_section(&kind).is_none() {
                let file = self.file.clone();
                self.errors.push(
                    CheckError::new(
                        E2302,
                        format!(
                            "unknown `@[bpf]` kind `{kind}`; supported: socket_filter, xdp, \
                             tracepoint, kprobe"
                        ),
                    )
                    .at(&file, 0, 0)
                    .with_span(f.span),
                );
            }
            // Every called builtin whose name starts with `bpf_` must be on the
            // capability allowlist. A non-allowlisted `bpf_*` helper is E2300.
            let mut bad_helpers: Vec<String> = Vec::new();
            Self::collect_bad_bpf_helpers(&f.body, &mut bad_helpers);
            for helper in bad_helpers {
                let file = self.file.clone();
                let allowed = crate::builtins::BPF_HELPER_BUILTINS.join(", ");
                self.errors.push(
                    CheckError::new(
                        E2300,
                        format!(
                            "BPF helper `{helper}` is not in the Axon capability allowlist; \
                             allowed: {allowed}"
                        ),
                    )
                    .at(&file, 0, 0)
                    .with_span(f.span)
                    .fix(format!(
                        "remove the call to `{helper}`, or add it to the BPF capability allowlist \
                         (BPF_HELPER_BUILTINS) with its helper id"
                    )),
                );
            }
        }
    }

    /// Collect calls in `expr` to a `bpf_*`-named function that is NOT on the
    /// allowlist (E2300). A `bpf_*` name that is not a known builtin at all is
    /// still flagged — it cannot be lowered to a verifiable BPF helper.
    fn collect_bad_bpf_helpers(expr: &Expr, out: &mut Vec<String>) {
        if let Expr::Call { callee, .. } = expr {
            if let Expr::Ident(name) = callee.as_ref() {
                if name.starts_with("bpf_") && !crate::builtins::is_bpf_helper(name) {
                    out.push(name.clone());
                }
            }
        }
        Self::for_each_child(expr, &mut |c| Self::collect_bad_bpf_helpers(c, out));
    }

    /// Collect heap-allocation sites inside a `@[no_alloc]` body: allocating
    /// builtin calls, interpolated `FmtStr`, and calls to (transitively)
    /// allocating user fns.
    fn collect_no_alloc_violations(
        expr: &Expr,
        allocating_fns: &HashSet<String>,
        out: &mut Vec<(String, &'static str)>,
    ) {
        match expr {
            Expr::FmtStr { parts } if parts.iter().any(|p| matches!(p, FmtPart::Expr(_))) => {
                out.push(("string interpolation".to_string(), "heap"));
            }
            Expr::Call { callee, .. } => {
                if let Expr::Ident(name) = callee.as_ref() {
                    if crate::builtins::is_heap_allocating_builtin(name) {
                        out.push((name.clone(), "allocating builtin"));
                    } else if allocating_fns.contains(name) {
                        out.push((name.clone(), "allocating function"));
                    }
                }
            }
            _ => {}
        }
        Self::for_each_child(expr, &mut |c| {
            Self::collect_no_alloc_violations(c, allocating_fns, out)
        });
    }

    /// R24 TEE (E1810): a NON-`@[enclave]` fn body may not call `tee_unseal`.
    /// `tee_unseal` is the in-enclave Secret-declassification primitive — it is
    /// the boundary where data you "can't read" outside the TEE becomes readable.
    /// Unsealing OUTSIDE the enclave region defeats the whole point, so the
    /// checker refuses it. Emits one E1810 per `tee_unseal` call site, naming the
    /// enclosing fn and pointing the author at `@[enclave]`.
    fn check_enclave_unseal(&mut self, fname: &str, body: &Expr, span: crate::span::Span) {
        let mut sites: usize = 0;
        Self::count_tee_unseal(body, &mut sites);
        for _ in 0..sites {
            let file = self.file.clone();
            self.errors.push(
                CheckError::new(
                    E1810,
                    format!(
                        "`tee_unseal` (Secret declassification) called from `{fname}`, which is not \
                         an `@[enclave]` function — a sealed Secret may only be unsealed INSIDE the \
                         trusted execution environment"
                    ),
                )
                .at(&file, 0, 0)
                .with_span(span)
                .fix(format!(
                    "annotate `{fname}` with `@[enclave]` so the unseal runs in-enclave, or move the \
                     `tee_unseal` call into an `@[enclave]` fn and pass the unsealed value out as a \
                     non-Secret result (the TEE boundary is the declassification point)"
                )),
            );
        }
    }

    /// Count direct `tee_unseal(...)` call sites in an expression tree. Lexical
    /// by design: a `tee_unseal` reached through a helper still trips E1810 on the
    /// HELPER (which must itself be `@[enclave]`), so there is no laundering path.
    fn count_tee_unseal(expr: &Expr, count: &mut usize) {
        if let Expr::Call { callee, .. } = expr {
            if let Expr::Ident(name) = callee.as_ref() {
                if name == "tee_unseal" {
                    *count += 1;
                }
            }
        }
        Self::for_each_child(expr, &mut |c| Self::count_tee_unseal(c, count));
    }

    /// Collect `x.m()` MethodCalls whose method name is in `impure_methods`
    /// (their impl body is impure). Pushed as ("m", "impure method") so the
    /// E1207 message reads uniformly with the Ident-call violations.
    fn collect_impure_method_calls(
        expr: &Expr,
        impure_methods: &HashSet<String>,
        out: &mut Vec<(String, &'static str)>,
    ) {
        if let Expr::MethodCall { method, .. } = expr {
            if impure_methods.contains(method) {
                out.push((method.clone(), "impure method"));
            }
        }
        Self::for_each_child(expr, &mut |c| {
            Self::collect_impure_method_calls(c, impure_methods, out)
        });
    }

    /// Phase 5/6: a refinement predicate must be PURE — it is a static,
    /// deterministic contract, so an impure builtin in it (now_ms, random_i64,
    /// I/O, AI, channels, …) makes the refinement non-deterministic and
    /// meaningless. Reject any impure builtin used in `pred` with E1209, naming
    /// the offending builtin. (User-fn calls in predicates are handled elsewhere;
    /// here we flag the impure-BUILTIN case, the one that silently slipped
    /// through — `now_ms() > 0` was accepted as a refinement.)
    fn reject_impure_refinement(&mut self, pred: &Expr, refine_name: &str) {
        let mut violations: Vec<(String, &'static str)> = Vec::new();
        // `pure_fns` empty here is fine: we only care about the impure-builtin
        // hits for a refinement (a non-pure user-fn call in a predicate is a
        // separate concern; the impure builtin is the soundness hole we close).
        Self::collect_purity_violations(pred, &HashSet::new(), &mut violations);
        for (callee, kind) in violations {
            if kind == "impure builtin" {
                let file = self.file.clone();
                self.errors.push(
                    CheckError::new(
                        E1209,
                        format!(
                            "refinement `{refine_name}` uses the impure builtin `{callee}` — a \
                             refinement predicate must be pure (deterministic), so it may not call \
                             I/O, AI, time, randomness, or channel builtins"
                        ),
                    )
                    .at(&file, 0, 0)
                    .fix(format!(
                        "remove `{callee}` from the predicate; a refinement must depend only on \
                         the value `_` and pure computation"
                    )),
                );
            }
        }
    }

    /// Recursively collect impure operations inside a `@[pure]` function body.
    /// `(name, kind)` where kind ∈ {"impure builtin", "non-pure function",
    /// "concurrency primitive"}.
    fn collect_purity_violations(
        expr: &Expr,
        pure_fns: &HashSet<String>,
        out: &mut Vec<(String, &'static str)>,
    ) {
        match expr {
            Expr::Call { callee, args, .. } => {
                if let Expr::Ident(name) = callee.as_ref() {
                    if crate::builtins::is_impure_builtin(name) {
                        out.push((name.clone(), "impure builtin"));
                    } else if crate::builtins::is_known_builtin(name) {
                        // a pure (or pure-allowlisted) builtin — fine.
                    } else if !pure_fns.contains(name) {
                        // A user function not marked `@[pure]` (P01). Unknown
                        // names are left to E0001; here we only flag a resolved
                        // user fn that exists but lacks `@[pure]`.
                        out.push((name.clone(), "non-pure function"));
                    }
                }
                Self::collect_purity_violations(callee, pure_fns, out);
                for a in args {
                    Self::collect_purity_violations(a, pure_fns, out);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                Self::collect_purity_violations(receiver, pure_fns, out);
                for a in args {
                    Self::collect_purity_violations(a, pure_fns, out);
                }
            }
            Expr::Spawn(inner) => {
                out.push(("spawn".to_string(), "concurrency primitive"));
                Self::collect_purity_violations(inner, pure_fns, out);
            }
            Expr::Select(_) => {
                out.push(("select".to_string(), "concurrency primitive"));
            }
            // Structural recursion over every child-bearing variant.
            Expr::Block(stmts) => {
                for s in stmts {
                    Self::collect_purity_violations(&s.expr, pure_fns, out);
                }
            }
            Expr::WithHandler { handler, body } => {
                if let crate::ast::HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                    for arm in arms.iter().chain(return_arm.as_deref()) {
                        Self::collect_purity_violations(&arm.body, pure_fns, out);
                    }
                }
                Self::collect_purity_violations(body, pure_fns, out);
            }
            Expr::Let { value, .. } | Expr::Own { value, .. } | Expr::RefBind { value, .. } => {
                Self::collect_purity_violations(value, pure_fns, out);
            }
            Expr::BinOp { left, right, .. } => {
                Self::collect_purity_violations(left, pure_fns, out);
                Self::collect_purity_violations(right, pure_fns, out);
            }
            Expr::UnaryOp { operand, .. } | Expr::Question(operand) | Expr::Comptime(operand) => {
                Self::collect_purity_violations(operand, pure_fns, out);
            }
            Expr::Match { subject, arms } => {
                Self::collect_purity_violations(subject, pure_fns, out);
                for arm in arms {
                    Self::collect_purity_violations(&arm.body, pure_fns, out);
                }
            }
            Expr::If { cond, then, else_ } => {
                Self::collect_purity_violations(cond, pure_fns, out);
                Self::collect_purity_violations(then, pure_fns, out);
                if let Some(e) = else_ {
                    Self::collect_purity_violations(e, pure_fns, out);
                }
            }
            Expr::Return(Some(inner)) => Self::collect_purity_violations(inner, pure_fns, out),
            Expr::FieldAccess { receiver, .. } => {
                Self::collect_purity_violations(receiver, pure_fns, out)
            }
            Expr::Index { receiver, index } => {
                Self::collect_purity_violations(receiver, pure_fns, out);
                Self::collect_purity_violations(index, pure_fns, out);
            }
            Expr::Tuple(es) | Expr::Array(es) => {
                for e in es {
                    Self::collect_purity_violations(e, pure_fns, out);
                }
            }
            Expr::Ok(inner) | Expr::Err(inner) | Expr::Some(inner) => {
                Self::collect_purity_violations(inner, pure_fns, out);
            }
            Expr::StructLit { fields, .. } => {
                for (_, fe) in fields {
                    Self::collect_purity_violations(fe, pure_fns, out);
                }
            }
            Expr::While { cond, body, .. } => {
                Self::collect_purity_violations(cond, pure_fns, out);
                for s in body {
                    Self::collect_purity_violations(&s.expr, pure_fns, out);
                }
            }
            Expr::WhileLet { expr, body, .. } => {
                Self::collect_purity_violations(expr, pure_fns, out);
                for s in body {
                    Self::collect_purity_violations(&s.expr, pure_fns, out);
                }
            }
            Expr::Assign { value, .. } => Self::collect_purity_violations(value, pure_fns, out),
            Expr::AssignTo { place, value } => {
                Self::collect_purity_violations(place, pure_fns, out);
                Self::collect_purity_violations(value, pure_fns, out);
            }
            Expr::For {
                start, end, body, ..
            } => {
                Self::collect_purity_violations(start, pure_fns, out);
                Self::collect_purity_violations(end, pure_fns, out);
                for s in body {
                    Self::collect_purity_violations(&s.expr, pure_fns, out);
                }
            }
            Expr::FmtStr { parts } => {
                for p in parts {
                    if let crate::ast::FmtPart::Expr(e) = p {
                        Self::collect_purity_violations(e, pure_fns, out);
                    }
                }
            }
            // AUDIT T29 (finding P4-FE-01). A lambda body is NOT a leaf: it is
            // code this function causes to run. Listing it here meant the
            // purity walker never looked inside one, so an impure operation
            // laundered through a closure passed clean:
            //
            //     @[pure] fn f(xs: &[i64]) -> i64 {
            //         arr_fold(xs, 0, |a, x| { println("boom") a + x })
            //     }
            //
            // checked exit 0, while the same `println` written directly in the
            // body is correctly E1207. Same shape as the @[contained] and
            // @[sensitive] laundering holes: a guard that inspects only the
            // immediate body is escapable by moving the work one hop.
            Expr::Lambda { body, .. } => {
                Self::collect_purity_violations(body, pure_fns, out);
            }
            // Leaves / no child exprs to walk.
            Expr::Ident(_)
            | Expr::Literal(_)
            | Expr::None
            | Expr::Break
            | Expr::Continue
            | Expr::Return(None)
            | Expr::InlineAsm { .. } => {}
        }
    }

    /// Phase 5 §3 — discharge the `@[total]` termination obligation. Collect
    /// every self-recursive call site; if there are none, the fn is trivially
    /// total (pass silently). Otherwise require ONE parameter index that
    /// strictly decreases at EVERY recursive call — an automatic well-founded
    /// measure. Two measure kinds are recognised (the rest of §3 needs the
    /// refinement/SMT machinery):
    ///   - i64 param: the recursive call passes `p - K` (K>0) or `p / K` (K>1),
    ///     i.e. a strictly-smaller derivation of the same parameter.
    ///   - slice param: the recursive call passes `tail(p)` / `arr_drop(p, K)` /
    ///     `arr_tail(p)` — a strictly-shorter slice.
    ///
    /// Mutual recursion is out of scope (only self-recursion is analysed). E1208
    /// when no single index decreases across all sites.
    fn check_totality(&mut self, f: &FnDef) {
        // A `while` loop is unbounded — its termination is not decidable in
        // general, and the totality analysis only reasons about recursion and
        // bounded `for` ranges. So a `@[total]` fn may not use `while`: it
        // claims termination the checker cannot establish (verified: a `@[total]`
        // fn with `while n < 10 { }` was accepted yet hangs forever). Require
        // bounded `for` loops or structural recursion instead.
        if Self::body_has_while(&f.body) {
            let file = self.file.clone();
            self.errors.push(
                CheckError::new(
                    E1208,
                    format!(
                        "`@[total]` function `{}` uses a `while` loop, whose termination cannot \
                         be established — a total function must use a bounded `for` range or \
                         structural recursion instead",
                        f.name
                    ),
                )
                .at(&file, 0, 0)
                .with_span(f.span)
                .fix(
                    "replace the `while` loop with a bounded `for i in a..b { … }` loop, or \
                      drop the `@[total]` attribute"
                        .to_string(),
                ),
            );
        }
        // A `@[total]` fn may only call other `@[total]` fns (and always-
        // terminating builtins). Otherwise non-termination launders through an
        // un-annotated helper: `@[total] f(){ loops() }` where `fn loops(){loops()}`
        // would pass (f has no self-recursion) yet never returns. A call to a
        // non-total USER fn is E1208. Self-calls are exempt (the measure check
        // below handles them); builtins are total. (Mutual recursion among total
        // fns is still out of scope — the measure check only sees self-calls — a
        // documented limit, but now the partners must at least be annotated.)
        let mut bad_callees: Vec<String> = Vec::new();
        Self::collect_nontotal_callees(&f.body, &f.name, &self.total_fns, &mut bad_callees);
        if let Some(callee) = bad_callees.first() {
            let file = self.file.clone();
            let fname = f.name.clone();
            self.errors.push(
                CheckError::new(
                    E1208,
                    format!(
                        "`@[total]` function `{fname}` calls `{callee}`, which is not `@[total]` — \
                         its termination is not established, so `{fname}` cannot be proven total"
                    ),
                )
                .at(&file, 0, 0)
                .with_span(f.span)
                .fix(format!(
                    "mark `{callee}` `@[total]` (the checker will verify it), or remove `@[total]` from `{fname}`"
                )),
            );
        }

        // Mutual recursion among `@[total]` fns: `a → b → a`. Each fn has NO
        // self-recursion, so the measure check below passes vacuously, yet the
        // pair loops forever. The per-fn decreasing-measure analysis can't span
        // two fns, so a cycle of length ≥2 is unprovable → refuse (E1208), the
        // same sound-by-refusal stance as `@[total]` + `while`. (A direct
        // self-cycle `a → a` is NOT flagged here — it's handled by the measure.)
        if self.total_reaches_through_other(&f.name) {
            let file = self.file.clone();
            let fname = f.name.clone();
            self.errors.push(
                CheckError::new(
                    E1208,
                    format!(
                        "`@[total]` function `{fname}` is part of a mutual-recursion cycle — \
                         the checker proves termination per-function, so it cannot establish that \
                         a cycle spanning multiple functions terminates"
                    ),
                )
                .at(&file, 0, 0)
                .with_span(f.span)
                .fix(
                    "restructure the mutual recursion into a single self-recursive `@[total]` \
                     function with a decreasing measure, or remove `@[total]`"
                        .to_string(),
                ),
            );
            return;
        }

        let mut sites: Vec<Vec<Expr>> = Vec::new();
        Self::collect_self_calls(&f.body, &f.name, &mut sites);
        if sites.is_empty() {
            return; // non-recursive → trivially total.
        }
        let arity = f.params.len();
        // A parameter index "works" if its argument strictly decreases at EVERY
        // recursive call site.
        let decreasing_idx = (0..arity).find(|&i| {
            sites.iter().all(|args| {
                args.get(i)
                    .map(|a| Self::arg_strictly_decreases(a, &f.params[i].name))
                    .unwrap_or(false)
            })
        });
        if decreasing_idx.is_none() {
            let file = self.file.clone();
            let fname = f.name.clone();
            self.errors.push(
                CheckError::new(
                    E1208,
                    format!(
                        "`@[total]` function `{fname}` has no strictly-decreasing measure at its \
                         recursive call(s) — the checker could not prove it terminates"
                    ),
                )
                .at(&file, 0, 0)
                .with_span(f.span)
                .fix(
                    "make a single argument strictly smaller at every recursive call \
                     (e.g. `n - 1` on an i64 parameter, or `tail(v)` on a slice parameter), \
                     or remove `@[total]`"
                        .to_string(),
                ),
            );
        }
    }

    /// Does `start` (a `@[total]` fn) reach ITSELF via a call path that passes
    /// through at least one OTHER total fn? (i.e. is it in a mutual-recursion
    /// cycle of length ≥2.) A direct self-cycle is excluded — that's the measure
    /// check's job.
    fn total_reaches_through_other(&self, start: &str) -> bool {
        let body = match self.total_fn_defs.get(start) {
            Some(b) => b,
            None => return false,
        };
        let mut first_hops = Vec::new();
        Self::collect_total_callees(body, &self.total_fns, &mut first_hops);
        for g in first_hops.iter().filter(|g| g.as_str() != start) {
            let mut visited = HashSet::new();
            if self.total_can_reach(g, start, &mut visited) {
                return true;
            }
        }
        false
    }

    /// Can `from` reach `target` by following total-fn call edges?
    fn total_can_reach(&self, from: &str, target: &str, visited: &mut HashSet<String>) -> bool {
        if from == target {
            return true;
        }
        if !visited.insert(from.to_string()) {
            return false;
        }
        if let Some(body) = self.total_fn_defs.get(from) {
            let mut callees = Vec::new();
            Self::collect_total_callees(body, &self.total_fns, &mut callees);
            for c in &callees {
                if self.total_can_reach(c, target, visited) {
                    return true;
                }
            }
        }
        false
    }

    /// Collect the names of called fns that ARE `@[total]` (the call-graph edges
    /// used for cycle detection). Includes self-edges (harmless — the visited set
    /// bounds traversal).
    fn collect_total_callees(expr: &Expr, total_fns: &HashSet<String>, out: &mut Vec<String>) {
        if let Expr::Call { callee, .. } = expr {
            if let Expr::Ident(n) = callee.as_ref() {
                if total_fns.contains(n) && !out.contains(n) {
                    out.push(n.clone());
                }
            }
        }
        Self::for_each_child(expr, &mut |c| {
            Self::collect_total_callees(c, total_fns, out)
        });
    }

    /// Collect the names of called USER functions that are NOT `@[total]` (and
    /// are not the enclosing fn `self_name`, whose recursion the measure check
    /// handles). A callee that is a builtin or a known total fn is fine; anything
    /// else is a termination-laundering risk. `known_builtin` callees are skipped
    /// (builtins always terminate).
    fn collect_nontotal_callees(
        expr: &Expr,
        self_name: &str,
        total_fns: &HashSet<String>,
        out: &mut Vec<String>,
    ) {
        if let Expr::Call { callee, .. } = expr {
            if let Expr::Ident(n) = callee.as_ref() {
                if n != self_name
                    && !total_fns.contains(n)
                    && !crate::builtins::is_known_builtin(n)
                    && !out.contains(n)
                {
                    out.push(n.clone());
                }
            }
        }
        Self::for_each_child(expr, &mut |c| {
            Self::collect_nontotal_callees(c, self_name, total_fns, out)
        });
    }

    /// Collect the argument lists of every self-recursive call to `name` in
    /// `expr`. Each entry is one call site's args.
    fn collect_self_calls(expr: &Expr, name: &str, out: &mut Vec<Vec<Expr>>) {
        if let Expr::Call { callee, args, .. } = expr {
            if matches!(callee.as_ref(), Expr::Ident(n) if n == name) {
                out.push(args.clone());
            }
        }
        Self::for_each_child(expr, &mut |c| Self::collect_self_calls(c, name, out));
    }

    /// True if `expr` contains a `while`/`while let` loop anywhere — used to
    /// reject `@[total]` functions, whose termination analysis cannot reason
    /// about unbounded loops (only recursion + bounded `for` ranges).
    fn body_has_while(expr: &Expr) -> bool {
        if matches!(expr, Expr::While { .. } | Expr::WhileLet { .. }) {
            return true;
        }
        let mut found = false;
        Self::for_each_child(expr, &mut |c| {
            if !found {
                found = Self::body_has_while(c);
            }
        });
        found
    }

    /// True if `arg` is a strictly-smaller derivation of the parameter `pname`:
    /// `pname - K` (K>0), `pname / K` (K>1), or a shortening builtin applied to
    /// `pname` (`tail`/`arr_tail`/`arr_drop`).
    fn arg_strictly_decreases(arg: &Expr, pname: &str) -> bool {
        match arg {
            // pname - positive-literal   /   pname / literal>1
            Expr::BinOp { op, left, right } => {
                let left_is_param = matches!(left.as_ref(), Expr::Ident(n) if n == pname);
                match op {
                    crate::ast::BinOp::Sub => left_is_param && Self::is_positive_int_literal(right),
                    crate::ast::BinOp::Div => {
                        left_is_param
                            && Self::int_literal_value(right)
                                .map(|v| v > 1)
                                .unwrap_or(false)
                    }
                    _ => false,
                }
            }
            // tail(pname) / arr_tail(pname) / arr_drop(pname, _)
            Expr::Call { callee, args, .. } => {
                if let Expr::Ident(fname) = callee.as_ref() {
                    let shortens = matches!(
                        fname.as_str(),
                        "tail" | "arr_tail" | "arr_drop" | "arr_skip"
                    );
                    let first_is_param = args.first().is_some_and(|a| {
                        let inner = match a {
                            Expr::UnaryOp {
                                op: crate::ast::UnaryOp::Ref,
                                operand,
                            } => operand.as_ref(),
                            other => other,
                        };
                        matches!(inner, Expr::Ident(n) if n == pname)
                    });
                    shortens && first_is_param
                } else {
                    false
                }
            }
            _ => false,
        }
    }

    fn is_positive_int_literal(e: &Expr) -> bool {
        Self::int_literal_value(e).map(|v| v > 0).unwrap_or(false)
    }

    /// The i64 value of an integer literal (handles `0 - n` only as non-literal).
    fn int_literal_value(e: &Expr) -> Option<i64> {
        match e {
            Expr::Literal(crate::ast::Literal::Int(n)) => Some(*n),
            _ => None,
        }
    }

    /// Apply `g` to every direct child expression of `expr` (one level). Shared
    /// recursion helper.
    /// SQLi-by-construction (E1210). Every `sql_query(template, params)` must take a
    /// string-LITERAL template, so user data can only enter as a bound `?` parameter.
    /// A template built by concatenation (`Expr::BinOp`), interpolation (`Expr::FmtStr`),
    /// or any non-`Literal::Str` value could let attacker input become SQL STRUCTURE —
    /// the definition of an injection — so it is refused. This is the same literal-only
    /// discipline the `@[contained]` path/host checks use, applied to the query sink, and
    /// it is checked everywhere (not gated on `@[contained]`). Emits with the enclosing
    /// fn's span (`Expr::Call` carries none).
    fn check_sql_safety(&mut self, body: &Expr, span: crate::span::Span) {
        let mut violations = 0usize;
        Self::collect_sql_injection(body, &mut violations);
        if violations == 0 {
            return;
        }
        let file = self.file.clone();
        for _ in 0..violations {
            self.errors.push(
                CheckError::new(
                    E1210,
                    "`sql_query` template must be a string literal — a template built by \
                     concatenation or interpolation lets user input become SQL structure \
                     (SQL injection)",
                )
                .at(&file, 0, 0)
                .with_span(span)
                .fix(
                    "keep the SQL as a literal with `?` placeholders and pass user data via \
                     the `params` array, e.g. sql_query(\"SELECT * FROM t WHERE id = ?\", [user_id])",
                ),
            );
        }
    }

    /// Count `sql_query` calls whose first argument (the template) is not a string
    /// literal. Static + recursive via `for_each_child` so a query laundered through a
    /// nested expression or impl-method body is still found.
    fn collect_sql_injection(expr: &Expr, out: &mut usize) {
        if let Expr::Call { callee, args, .. } = expr {
            if let Expr::Ident(name) = callee.as_ref() {
                if name == "sql_query"
                    && !matches!(
                        args.first(),
                        Some(Expr::Literal(crate::ast::Literal::Str(_)))
                    )
                {
                    *out += 1;
                }
            }
        }
        Self::for_each_child(expr, &mut |c| Self::collect_sql_injection(c, out));
    }

    fn for_each_child(expr: &Expr, g: &mut dyn FnMut(&Expr)) {
        match expr {
            Expr::Block(stmts) => stmts.iter().for_each(|s| g(&s.expr)),
            Expr::Let { value, .. }
            | Expr::Own { value, .. }
            | Expr::RefBind { value, .. }
            | Expr::Question(value)
            | Expr::Comptime(value)
            | Expr::Spawn(value)
            | Expr::Assign { value, .. } => g(value),
            Expr::UnaryOp { operand, .. } => g(operand),
            Expr::Call { callee, args, .. } => {
                g(callee);
                for a in args {
                    g(a);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                g(receiver);
                for a in args {
                    g(a);
                }
            }
            Expr::BinOp { left, right, .. } => {
                g(left);
                g(right);
            }
            Expr::Match { subject, arms } => {
                g(subject);
                arms.iter().for_each(|a| {
                    // Walk the arm GUARD too, not just the body — a guard is a full
                    // expression and can hide a relevant call (e.g. a non-literal
                    // `sql_query` template → E1210, or a `while` under @[total]). Every
                    // for_each_child consumer inherits this; omitting it was a
                    // walker-coverage hole (the SQLi red-team found it via a match guard).
                    if let Some(guard) = &a.guard {
                        g(guard);
                    }
                    g(&a.body);
                });
            }
            Expr::If { cond, then, else_ } => {
                g(cond);
                g(then);
                if let Some(e) = else_ {
                    g(e);
                }
            }
            Expr::Return(Some(inner)) | Expr::Ok(inner) | Expr::Err(inner) | Expr::Some(inner) => {
                g(inner)
            }
            Expr::FieldAccess { receiver, .. } => g(receiver),
            Expr::Index { receiver, index } => {
                g(receiver);
                g(index);
            }
            Expr::Tuple(es) | Expr::Array(es) => {
                for e in es {
                    g(e);
                }
            }
            Expr::StructLit { fields, .. } => fields.iter().for_each(|(_, fe)| g(fe)),
            Expr::While { cond, body, .. } => {
                g(cond);
                body.iter().for_each(|s| g(&s.expr));
            }
            Expr::WhileLet { expr, body, .. } => {
                g(expr);
                body.iter().for_each(|s| g(&s.expr));
            }
            Expr::For {
                start, end, body, ..
            } => {
                g(start);
                g(end);
                body.iter().for_each(|s| g(&s.expr));
            }
            Expr::AssignTo { place, value } => {
                g(place);
                g(value);
            }
            Expr::Select(arms) => {
                for arm in arms {
                    g(&arm.recv);
                    g(&arm.body);
                }
            }
            Expr::FmtStr { parts } => parts.iter().for_each(|p| {
                if let crate::ast::FmtPart::Expr(e) = p {
                    g(e);
                }
            }),
            Expr::WithHandler { handler, body } => {
                if let crate::ast::HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                    for arm in arms.iter().chain(return_arm.as_deref()) {
                        g(&arm.body);
                    }
                }
                g(body);
            }
            // A lambda's body is a child too — walking it lets analyses (e.g. the
            // @[total] while-check, recursion collection) see constructs hidden
            // inside a closure. Previously skipped, which let a `while` in a
            // lambda escape the @[total] termination check.
            Expr::Lambda { body, .. } => g(body),
            Expr::Ident(_)
            | Expr::Literal(_)
            | Expr::None
            | Expr::Break
            | Expr::Continue
            | Expr::Return(None)
            | Expr::InlineAsm { .. } => {}
        }
    }

    /// Pre-walk an expression tree, recording every identifier name `x` where
    /// `x.confidence` (or a chained `x.foo.confidence`) is read. Conservative —
    /// false positives on collisions across scopes are acceptable for W0701.
    fn collect_confidence_observed(expr: &Expr, out: &mut HashSet<String>) {
        match expr {
            Expr::FieldAccess { receiver, field } => {
                if field == "confidence" {
                    if let Expr::Ident(name) = receiver.as_ref() {
                        out.insert(name.clone());
                    }
                }
                Self::collect_confidence_observed(receiver, out);
            }
            Expr::Block(stmts) => {
                for s in stmts {
                    Self::collect_confidence_observed(&s.expr, out);
                }
            }
            Expr::If { cond, then, else_ } => {
                Self::collect_confidence_observed(cond, out);
                Self::collect_confidence_observed(then, out);
                if let Some(e) = else_ {
                    Self::collect_confidence_observed(e, out);
                }
            }
            Expr::Match { subject, arms } => {
                Self::collect_confidence_observed(subject, out);
                for arm in arms {
                    Self::collect_confidence_observed(&arm.body, out);
                }
            }
            Expr::Call { callee, args, .. } => {
                Self::collect_confidence_observed(callee, out);
                for a in args {
                    Self::collect_confidence_observed(a, out);
                }
            }
            Expr::MethodCall { receiver, args, .. } => {
                Self::collect_confidence_observed(receiver, out);
                for a in args {
                    Self::collect_confidence_observed(a, out);
                }
            }
            Expr::BinOp { left, right, .. } => {
                Self::collect_confidence_observed(left, out);
                Self::collect_confidence_observed(right, out);
            }
            Expr::UnaryOp { operand, .. } => Self::collect_confidence_observed(operand, out),
            Expr::While { cond, body } => {
                Self::collect_confidence_observed(cond, out);
                for s in body {
                    Self::collect_confidence_observed(&s.expr, out);
                }
            }
            Expr::WhileLet { expr, body, .. } => {
                Self::collect_confidence_observed(expr, out);
                for s in body {
                    Self::collect_confidence_observed(&s.expr, out);
                }
            }
            Expr::For {
                start, end, body, ..
            } => {
                Self::collect_confidence_observed(start, out);
                Self::collect_confidence_observed(end, out);
                for s in body {
                    Self::collect_confidence_observed(&s.expr, out);
                }
            }
            Expr::Index { receiver, index } => {
                Self::collect_confidence_observed(receiver, out);
                Self::collect_confidence_observed(index, out);
            }
            Expr::Spawn(inner) | Expr::Comptime(inner) => {
                Self::collect_confidence_observed(inner, out);
            }
            Expr::Lambda { body, .. } => Self::collect_confidence_observed(body, out),
            Expr::Ok(inner) | Expr::Err(inner) | Expr::Some(inner) => {
                Self::collect_confidence_observed(inner, out);
            }
            Expr::FmtStr { parts } => {
                for part in parts {
                    if let crate::ast::FmtPart::Expr(e) = part {
                        Self::collect_confidence_observed(e, out);
                    }
                }
            }
            Expr::Array(elems) | Expr::Tuple(elems) => {
                for e in elems {
                    Self::collect_confidence_observed(e, out);
                }
            }
            Expr::StructLit { fields, .. } => {
                for (_n, e) in fields {
                    Self::collect_confidence_observed(e, out);
                }
            }
            Expr::Select(arms) => {
                for arm in arms {
                    Self::collect_confidence_observed(&arm.recv, out);
                    Self::collect_confidence_observed(&arm.body, out);
                }
            }
            Expr::Question(inner) => Self::collect_confidence_observed(inner, out),
            Expr::Let { value, .. } | Expr::Own { value, .. } | Expr::RefBind { value, .. } => {
                Self::collect_confidence_observed(value, out);
            }
            Expr::Assign { value, .. } => Self::collect_confidence_observed(value, out),
            Expr::Return(Some(e)) => {
                Self::collect_confidence_observed(e, out);
            }
            // Leaf / no-receiver variants
            _ => {}
        }
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Statement
    // ─────────────────────────────────────────────────────────────────────────

    fn check_stmt(&mut self, stmt: &Stmt, node_path: &str, scope: &mut HashMap<String, Type>) {
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

    fn check_expr(&mut self, expr: &Expr, node_path: &str, scope: &mut HashMap<String, Type>) {
        match expr {
            // ── Block ────────────────────────────────────────────────────────
            Expr::Block(stmts) => {
                let n = stmts.len();
                // Unreachable-code (W0005): once a statement unconditionally
                // diverts control (`return`/`break`/`continue`), every following
                // statement in this block is dead. Warn ONCE on the first one.
                if let Some(term_idx) = stmts
                    .iter()
                    .take(n.saturating_sub(1))
                    .position(|s| is_terminator(&s.expr))
                {
                    if term_idx + 1 < n {
                        let file = self.file.clone();
                        self.errors.push(
                            CheckError::warning(
                                crate::error::W0005,
                                "unreachable code: this and the following statements can never run (an earlier `return`/`break`/`continue` always diverts control)".to_string(),
                            )
                            .node(format!("{node_path}.stmt_{}", term_idx + 1))
                            .at(&file, 0, 0)
                            .fix("remove the dead code, or move it before the diverting statement".to_string()),
                        );
                    }
                }
                for (i, stmt) in stmts.iter().enumerate() {
                    let stmt_path = format!("{node_path}.stmt_{i}");
                    let is_last = i + 1 == n;

                    if is_last {
                        self.check_stmt(stmt, &stmt_path, scope);
                        // R07: the final expression of the function body must
                        // match the declared return type. Gate on the EXACT
                        // function-body path — `ends_with(".body")` also matches
                        // match-arm bodies (`…arm_N.body`), which would compare an
                        // arm's tail against the fn return type and false-flag.
                        if node_path == self.fn_body_path {
                            let expr_ty = self.resolve_expr_type(&stmt.expr, &stmt_path, scope);
                            self.check_return_type_match(&expr_ty, node_path, &stmt_path);
                            // R03: the body-tail (implicit return) into a
                            // refinement return type must satisfy the predicate.
                            self.check_return_refinement(&stmt.expr, &stmt_path);
                        }
                    } else {
                        // Non-final statement: R02 if a call returns Result
                        // and the result is not being stored or propagated.
                        self.check_stmt_result_ignored(&stmt.expr, &stmt_path, scope);
                        self.check_stmt(stmt, &stmt_path, scope);
                    }
                }
            }

            // Phase 6: `with <handler> { body }`. Check the handler arm bodies
            // and the wrapped body (the handler is otherwise inert in this slice).
            Expr::WithHandler { handler, body } => {
                if let crate::ast::HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                    for (i, arm) in arms.iter().chain(return_arm.as_deref()).enumerate() {
                        self.check_expr(&arm.body, &format!("{node_path}.handler_arm_{i}"), scope);
                    }
                }
                self.check_expr(body, &format!("{node_path}.with_body"), scope);
            }

            // ── Binding forms ────────────────────────────────────────────────
            // The RHS is "used" (stored), so R02 does not apply.
            Expr::Let {
                name,
                value,
                ty: annot,
            }
            | Expr::Own {
                name,
                value,
                ty: annot,
            }
            | Expr::RefBind {
                name,
                value,
                ty: annot,
            } => {
                let val_path = format!("{node_path}.value");
                self.check_expr(value, &val_path, scope);
                // Phase 5: a `let p: T where P = <const>` annotation carries a
                // refinement obligation — discharge it for a constant value
                // (E1209), mirroring the param/return/field constant checks. A
                // non-constant value defers to the runtime check (interp/codegen).
                self.check_let_refinement(annot, name, value, &val_path);
                // R19 Slice B: when the annotation names a non-i64 fixed-width
                // integer type (u8, u16, u32, u64, i8, i16, i32), use the ANNOTATION
                // type for the scope binding rather than resolving the value's
                // inferred type. Without this, `let a: u8 = 60` would bind `a: I64`
                // in the checker's scope (because the literal resolves to I64), and a
                // later `fn_taking_u8(a)` would emit a spurious E0306.
                let ty = if let Some(annot_type) = annot.as_ref().and_then(|a| {
                    if let crate::ast::AxonType::Named(n) = a {
                        match n.as_str() {
                            "u8" | "u16" | "u32" | "u64" | "i8" | "i16" | "i32" => {
                                Type::from_name(n)
                            }
                            _ => None,
                        }
                    } else {
                        None
                    }
                }) {
                    annot_type
                } else {
                    self.resolve_expr_type(value, &val_path, scope)
                };
                scope.insert(name.clone(), ty);
                // R6 local taint: if this binds a sensitive value (a sensitive
                // struct, or a field of one — `let e = u.email`), remember that
                // the LOCAL is now tainted, so a later `sink(e)` is caught even
                // though `e`'s static type is a plain scalar. (Only when some
                // type is sensitive — otherwise the map stays empty.)
                if !self.sensitive_types.is_empty() {
                    // Direct sensitive value (`let e = u.email`), OR a container
                    // literal that EMBEDS a sensitive value (`let w = Wrapper {
                    // data: u.email }`, `let t = (u.email, _)`). The container case
                    // over-approximates (the whole local is tainted), which is the
                    // safe direction for a security check — a later `w.data` /
                    // `t.0` extraction is then flagged.
                    // `sensitive_flow_in` already recurses into struct/array/tuple
                    // literals (case (c)), so a container that embeds a sensitive
                    // value taints the whole local (over-approximation, the safe
                    // direction). A later `w.data` / `t.0` extraction on the
                    // tainted local is then caught by the field/index walk in (a0).
                    match self.sensitive_flow_in(value, &val_path, scope) {
                        Some(src) => {
                            self.sensitive_locals.insert(name.clone(), src);
                        }
                        // A rebind to a NON-sensitive value clears any prior taint
                        // on this name (`let e = u.email; let e = "public"`), so a
                        // later use of the re-bound `e` is not falsely flagged.
                        None => {
                            self.sensitive_locals.remove(name);
                        }
                    }
                }
            }

            // ── Call ─────────────────────────────────────────────────────────
            Expr::Call { callee, args, .. } => {
                // Phase 13 Slice 2: P(dist op k) — skip checking args entirely;
                // the argument is a comparison with a distribution struct on the
                // left which does not type-check via the normal comparison rules.
                if let Expr::Ident(name) = callee.as_ref() {
                    if crate::builtins::is_prob_pred_ident(name) {
                        return; // skip all further checks for prob predicates
                    }
                }
                self.check_expr(callee, &format!("{node_path}.callee"), scope);
                for (i, arg) in args.iter().enumerate() {
                    self.check_expr(arg, &format!("{node_path}.arg_{i}"), scope);
                }
                // R13 native FFI: `M::fn(...)` argument boundary checks
                // (E1801 non-representable, E1802 cross-module handle).
                if let Expr::StructLit { name, fields } = callee.as_ref() {
                    if fields.is_empty() {
                        if let Some((_m, nf)) = crate::native::resolve_call(name) {
                            self.check_native_call_args(nf, args, node_path, scope);
                        }
                    }
                }
                // PRD §4 (privacy): a `@[sensitive]` value may not cross the
                // program boundary. Catches an arg to an EXFILTRATION sink — an
                // AI call (to a model), `write_file` (to disk), or `exec` (to a
                // process) — that is either (a) a value of a sensitive struct
                // type, or (b) a field of a sensitive struct / a field whose
                // type is sensitive → E1206.
                if let Expr::Ident(name) = callee.as_ref() {
                    if !self.sensitive_types.is_empty() {
                        // A direct builtin sink taints EVERY argument position; a
                        // user fn known to exfiltrate taints only the positions
                        // whose param reaches a sink (transitive taint).
                        let builtin_boundary = exfiltration_sink_kind(name);
                        let user_sink_positions = self.exfiltrating_params.get(name);
                        if builtin_boundary.is_some() || user_sink_positions.is_some() {
                            for (i, arg) in args.iter().enumerate() {
                                let is_sink_pos = builtin_boundary.is_some()
                                    || user_sink_positions.is_some_and(|s| s.contains(&i));
                                if !is_sink_pos {
                                    continue;
                                }
                                let apath = format!("{node_path}.arg_{i}");
                                if let Some((sname, cat)) =
                                    self.sensitive_flow_in(arg, &apath, scope)
                                {
                                    let file = self.file.clone();
                                    // For a builtin the boundary names the sink
                                    // kind; for a user fn it's an indirect leak
                                    // (the fn forwards this arg to a sink).
                                    let boundary =
                                        builtin_boundary.unwrap_or("exfiltrating function");
                                    let detail = if builtin_boundary.is_some() {
                                        format!(
                                            "the {boundary} `{name}` — sensitive data must never leave \
                                             the program boundary"
                                        )
                                    } else {
                                        format!(
                                            "`{name}`, which forwards argument {i} to an exfiltration \
                                             sink (AI call / file write / exec) — sensitive data must \
                                             never leave the program boundary"
                                        )
                                    };
                                    self.errors.push(
                                        CheckError::new(
                                            E1206,
                                            format!(
                                                "a `@[sensitive({cat})]` value from `{sname}` is passed to {detail}"
                                            ),
                                        )
                                        .node(&apath)
                                        .at(&file, 0, 0)
                                        .fix(format!(
                                            "strip the sensitive fields before the call (e.g. build a redacted \
                                             projection of `{sname}`), or move the call behind a local-only boundary"
                                        )),
                                    );
                                }
                            }
                        }
                    }
                }

                // R05 / R06 — only for named (direct) calls.
                if let Expr::Ident(name) = callee.as_ref() {
                    // Fix #3: detect calling a local variable that is not a function.
                    if let Some(ty) = scope.get(name.as_str()) {
                        if !matches!(
                            ty,
                            Type::Fn(_, _) | Type::Unknown | Type::Deferred(_) | Type::Var(_)
                        ) {
                            let file = self.file.clone();
                            self.errors.push(
                                CheckError::new(
                                    E0306,
                                    format!("cannot call non-function value '{name}'"),
                                )
                                .node(node_path)
                                .at(&file, 0, 0)
                                .fix(format!(
                                    "'{name}' is a local variable of type {}, not a function",
                                    ty.display()
                                )),
                            );
                        }
                    }
                    self.check_call_arity_and_types(name, args, node_path, scope);
                    self.check_refinement_args(name, args, node_path);
                }
            }

            // ── MethodCall ───────────────────────────────────────────────────
            Expr::MethodCall {
                receiver,
                method,
                args,
            } => {
                let rpath = format!("{node_path}.receiver");
                self.check_expr(receiver, &rpath, scope);
                for (i, arg) in args.iter().enumerate() {
                    self.check_expr(arg, &format!("{node_path}.arg_{i}"), scope);
                }
                // `recv.method(args)` is only valid when `method` is a method
                // defined on the receiver's type (via an impl block) or a builtin
                // channel method. Calling anything else — a data field `p.x()`, an
                // `Option`/`Result` (`o.unwrap()`), or a bare type with no such
                // impl (`n.foo()`, `[1].push()`, `"s".upper()`) — was silently
                // accepted at check time then panicked at runtime ("no method `m`
                // on type `T`"). Flag E0403 when the receiver resolves to a
                // concrete type whose `type_methods` set lacks `method`.
                //
                // Keyed by the SAME name the interpreter uses to register/look up
                // methods (Value::type_name() / type_name_of()), so a user impl on
                // a primitive (`impl Trait for i64`) is correctly recognized.
                let recv_ty = self.resolve_expr_type(receiver, &rpath, scope);
                let method_key: Option<String> = match &recv_ty {
                    Type::Struct(n) | Type::Enum(n) => Some(n.clone()),
                    other => method_lookup_key(other).map(|s| s.to_string()),
                };
                if let Some(key) = method_key {
                    let key = key.as_str();
                    let has_method = self
                        .type_methods
                        .get(key)
                        .is_some_and(|ms| ms.contains(method));
                    if !has_method {
                        let file = self.file.clone();
                        // Tailor the message + hint to the receiver shape.
                        let is_data_field = matches!(&recv_ty, Type::Struct(s)
                            if self.struct_fields.get(s).is_some_and(|fs| fs.iter().any(|(n,_)| n == method)));
                        let (msg, hint): (String, String) = if is_data_field {
                            (
                                format!("`{method}` is a data field of `{key}`, not a method — it cannot be called"),
                                format!("remove the call parentheses to read the field: `…{method}`"),
                            )
                        } else if matches!(recv_ty, Type::Option(_)) {
                            (
                                format!("`Option` has no method `{method}` — Axon has no null/unwrap; Option is destructured by matching"),
                                "match on it instead: `match opt { Some(v) => …  None => … }`".to_string(),
                            )
                        } else if matches!(recv_ty, Type::Result(_, _)) {
                            (
                                format!("`Result` has no method `{method}` — Axon has no null/unwrap; Result is destructured by matching"),
                                "match on it instead: `match res { Ok(v) => …  Err(e) => … }`".to_string(),
                            )
                        } else {
                            (
                                format!("no method `{method}` on type `{}`", recv_ty.display()),
                                format!("define it with `impl SomeTrait for {key} {{ fn {method}(self: {key}) … }}`, or call a free function"),
                            )
                        };
                        self.errors.push(
                            CheckError::new(E0403, msg)
                                .node(node_path)
                                .at(&file, 0, 0)
                                .fix(hint),
                        );
                    } else if let Some(sig) = self.fn_sigs.get(&format!("{key}__{method}")).cloned()
                    {
                        // ARITY check for the impl method (was MISSING — a wrong
                        // arg count passed both infer and the checker, then panicked
                        // at runtime "expected N args, got M"). Method sigs include
                        // `self` as param 0, so the explicit call args map to
                        // params[1..]. Arity ONLY — type-checking `self` across impls
                        // is fragile (skip it to avoid false positives). If the
                        // mangled sig isn't found we simply don't check (no FP).
                        let expected = sig.params.len().saturating_sub(1);
                        if args.len() != expected {
                            let file = self.file.clone();
                            let span = self.current_span;
                            self.errors.push(
                                CheckError::new(
                                    E0305,
                                    format!(
                                        "method `{method}` on `{key}` takes {expected} argument{} but {} {} supplied",
                                        if expected == 1 { "" } else { "s" },
                                        args.len(),
                                        if args.len() == 1 { "was" } else { "were" },
                                    ),
                                )
                                .node(node_path)
                                .at(&file, 0, 0)
                                .with_span(span)
                                .expected(expected.to_string())
                                .found(args.len().to_string())
                                .fix(format!(
                                    "`{method}` takes {expected} argument(s) besides the receiver `self`"
                                )),
                            );
                        }
                    }
                    // @[sensitive] exfiltration through a METHOD: if `key__method`
                    // forwards a parameter to a sink (computed by the taint fixpoint,
                    // which now covers impl methods), and the matching explicit arg
                    // is a sensitive value, it's E1206 — the MethodCall analog of the
                    // Call-site check (was missing entirely). Method param idx `p`
                    // includes `self` at 0, so the explicit arg is at `p - 1`.
                    if has_method && !self.sensitive_types.is_empty() {
                        if let Some(positions) = self
                            .exfiltrating_params
                            .get(&format!("{key}__{method}"))
                            .cloned()
                        {
                            for p in positions {
                                if p == 0 {
                                    continue; // the receiver (`self`) slot
                                }
                                if let Some(arg) = args.get(p - 1) {
                                    let apath = format!("{node_path}.arg_{}", p - 1);
                                    if let Some((sname, cat)) =
                                        self.sensitive_flow_in(arg, &apath, scope)
                                    {
                                        let file = self.file.clone();
                                        let ai = p - 1;
                                        self.errors.push(
                                            CheckError::new(
                                                E1206,
                                                format!(
                                                    "a `@[sensitive({cat})]` value from `{sname}` is passed to `{method}`, \
                                                     which forwards argument {ai} to an exfiltration sink (AI call / file \
                                                     write / exec) — sensitive data must never leave the program boundary"
                                                ),
                                            )
                                            .node(&apath)
                                            .at(&file, 0, 0)
                                            .fix(format!(
                                                "strip the sensitive fields before the call (e.g. build a redacted \
                                                 projection of `{sname}`), or move the call behind a local-only boundary"
                                            )),
                                        );
                                    }
                                }
                            }
                        }
                    }
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
                if matches!(
                    op,
                    BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem
                ) {
                    // R13 E1803: arithmetic on a native handle is forbidden
                    // (opaque, unforgeable). Checked BEFORE check_numeric_operand,
                    // which short-circuits on Deferred (handles are Deferred).
                    self.check_handle_not_arithmetic(&lty, &lpath, "used in arithmetic");
                    self.check_handle_not_arithmetic(&rty, &rpath, "used in arithmetic");
                    // N2a: `str + str` is CONCATENATION, and the interpreter has
                    // implemented it all along (`interp/value.rs:681`
                    // `(Add, Str(a), Str(b)) => Ok(Str(a + &b))`). Only this
                    // check refused it, so the evaluator arm was unreachable —
                    // the checker being more restrictive than the reference
                    // oracle, which is the same shape as M4 in the opposite
                    // direction.
                    //
                    // Deliberately narrow: BOTH sides must be `str`, and only
                    // for `Add`. `"a" - "b"` stays refused here.
                    //
                    // The `rty` half is belt-and-braces, and mutation-testing
                    // showed why that is worth saying rather than assuming:
                    // loosening this to `lty == Str` alone did NOT fail the
                    // mixed-operand test, because INFER already refuses
                    // `"a" + 1` by unification ("type mismatch in arithmetic
                    // operands"), a different E0102 from this one. So that test
                    // covers infer, not this clause. The clause stays because it
                    // makes this check's intent independent of another pass —
                    // if unification ever loosened, `str + int` must not become
                    // silently legal here.
                    let str_concat =
                        matches!(op, BinOp::Add) && lty == Type::Str && rty == Type::Str;
                    // N2b: same permission for `[T] + [T]`. Element types must
                    // already agree — infer unifies them — so this only has to
                    // stop the numeric check from firing on two arrays.
                    let arr_concat = matches!(op, BinOp::Add)
                        && matches!(lty, Type::Slice(_))
                        && matches!(rty, Type::Slice(_));
                    if !str_concat && !arr_concat {
                        self.check_numeric_operand(&lty, &lpath);
                        self.check_numeric_operand(&rty, &rpath);
                    }
                }
                // Integer division/remainder by a divisor that CONSTANT-FOLDS to
                // zero always panics at runtime ("integer division by zero").
                // Catch it statically as E0407 — covers a literal `0` AND a
                // constant integer expression like `(2 - 2)` / `(0 * 5)`. A float
                // `/0.0` is `inf` (not a panic) and a non-constant divisor can't
                // be known here, so both are left alone.
                if matches!(op, BinOp::Div | BinOp::Rem)
                    && const_eval_int(right.as_ref()) == Some(0)
                {
                    let file = self.file.clone();
                    let verb = if matches!(op, BinOp::Div) {
                        "division"
                    } else {
                        "remainder"
                    };
                    self.errors.push(
                        CheckError::new(
                            E0407,
                            format!("integer {verb} by zero — this always panics at runtime"),
                        )
                        .node(node_path)
                        .at(&file, 0, 0)
                        .fix(
                            "guard the divisor (`if d != 0 { … }`) or use a non-zero constant"
                                .to_string(),
                        ),
                    );
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
                    // R13 E1803: cannot negate a native handle.
                    self.check_handle_not_arithmetic(&ty, &opath, "used in arithmetic");
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
                        self.check_expr(guard, &format!("{node_path}.arm_{i}.guard"), scope);
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
                    // R03: a constant return into a refinement return type must
                    // satisfy the predicate.
                    self.check_return_refinement(v, &vpath);
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

                // ── W0701: uncertainty discarded ───────────────────────────
                // If `.value` is read on an `Uncertain<T>` and the enclosing
                // function never inspects the same identifier's `.confidence`,
                // emit a Layer-1 informational warning. Heuristic — suppression
                // is identifier-name-keyed, so chained / aliased forms may
                // false-positive (acceptable per spec).
                if field == "value" && matches!(recv_ty, Type::Uncertain(_)) {
                    if let Expr::Ident(name) = receiver.as_ref() {
                        if !self.confidence_observed.contains(name) {
                            let file = self.file.clone();
                            self.errors.push(
                                CheckError::warning(
                                    crate::error::W0701,
                                    format!(
                                        "uncertainty discarded: '{name}.value' read without checking '{name}.confidence' in this function",
                                    ),
                                )
                                .node(node_path)
                                .at(&file, 0, 0)
                                .fix("guard with `if {ident}.confidence > THRESHOLD {{ ... }}` before reading `.value`"),
                            );
                        }
                    }
                }
            }

            // ── Index ────────────────────────────────────────────────────────
            Expr::Index { receiver, index } => {
                // Phase 13 Slice 2: E[dist] and Var[dist] are distribution moment
                // predicates — skip all array/I64 constraints for them.
                if let Expr::Ident(tag) = receiver.as_ref() {
                    if crate::builtins::is_prob_pred_ident(tag) {
                        self.check_expr(index, &format!("{node_path}.index"), scope);
                        return;
                    }
                }
                let recv_path = format!("{node_path}.receiver");
                self.check_expr(receiver, &recv_path, scope);
                self.check_expr(index, &format!("{node_path}.index"), scope);
                // R11-sibling: indexing is only valid on a slice/array. A
                // concrete non-slice receiver (`n[0]` where `n: i64`, or a bool,
                // or a struct) used to be silently accepted at check time and
                // panic at runtime ("indexing non-array (i64)"). Flag it as
                // E0402. Skip Unknown/Var/Deferred (let inference own those) and
                // tuples (those are accessed via `.0`/`.1`, reported elsewhere).
                let recv_ty = self.resolve_expr_type(receiver, &recv_path, scope);
                // R13 E1803: indexing a native handle is forging — forbidden.
                // (Checked before the `is_deferred()` indexable short-circuit,
                // which would otherwise silently accept `handle[i]`.)
                self.check_handle_not_arithmetic(&recv_ty, &recv_path, "indexed");
                let indexable = matches!(recv_ty, Type::Slice(_))
                    || recv_ty.is_deferred()
                    || matches!(recv_ty, Type::Unknown | Type::Var(_));
                if !indexable {
                    let file = self.file.clone();
                    self.errors.push(
                        CheckError::new(
                            E0402,
                            format!("cannot index a value of type {}", recv_ty.display()),
                        )
                        .node(node_path)
                        .at(&file, 0, 0)
                        .fix("indexing `a[i]` is only valid on an array/slice `[T]`"),
                    );
                }
            }

            // ── Spawn / Comptime ─────────────────────────────────────────────
            Expr::Spawn(inner) | Expr::Comptime(inner) => {
                self.check_expr(inner, &format!("{node_path}.inner"), scope);
            }

            // ── Inline asm (R17 Slice 1) ─────────────────────────────────────
            Expr::InlineAsm { .. } => {
                // Type is Unit; no sub-expressions to walk. Hardware-only:
                // the interpreter refuses this at runtime (E0910).
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
            Expr::Lambda {
                params: _,
                body,
                captures: _,
            } => {
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
            Expr::For {
                var,
                start,
                end,
                body,
                ..
            } => {
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
                // A reassignment does NOT change the variable's type (Axon vars are
                // single-type; a type-changing reassignment is an infer-level error).
                // Crucially, do NOT overwrite a known declared type with `Unknown`
                // when the RHS is a BinOp / lambda / unknown-call (all of which
                // resolve to `Unknown` in the checker's syntactic fallback) — that
                // erasure made every downstream structural check (field access,
                // arity, option-as-value) silently SKIP the variable after a
                // reassignment like `x = x + 1`, missing real errors (e.g. `x.foo`
                // on an i64). Keep the prior type when the new one is Unknown.
                if !matches!(ty, Type::Unknown) || !scope.contains_key(name) {
                    scope.insert(name.clone(), ty);
                }
            }
            Expr::AssignTo { place, value } => {
                self.check_expr(place, &format!("{node_path}.place"), scope);
                self.check_expr(value, &format!("{node_path}.value"), scope);
            }

            // ── FmtStr: check each interpolated sub-expression ──────────────
            Expr::FmtStr { parts } => {
                for part in parts {
                    if let FmtPart::Expr(e) = part {
                        self.check_expr(e, &format!("{node_path}.fmt_part"), scope);
                    }
                }
            }

            // ── StructLit (incl. enum-variant literals `Enum::Variant {..}`) ──
            Expr::StructLit { name, fields } => {
                // Check field VALUE expressions (infer owns field-name validation
                // for structs via E0101; this walks nested exprs for other rules).
                for (i, (_fname, fexpr)) in fields.iter().enumerate() {
                    self.check_expr(fexpr, &format!("{node_path}.field_{i}"), scope);
                }
                // R04: a constant field value whose declared field type is a
                // refinement must satisfy the predicate.
                if let Some(field_refs) = self.struct_field_refinements.get(name).cloned() {
                    for (i, (fname, fexpr)) in fields.iter().enumerate() {
                        let Some(rname) = field_refs.get(fname) else {
                            continue;
                        };
                        self.check_field_refinement(
                            rname,
                            fname,
                            name,
                            fexpr,
                            &format!("{node_path}.field_{i}"),
                        );
                    }
                }
                // Phase 5: a WHOLE-struct refinement (`{…} where _.lo <= _.hi`) —
                // if every field value is a compile-time constant, build the
                // struct binder and evaluate the predicate; a provably-false one
                // is E1209.
                if let Some(pred) = self.struct_refinements.get(name).cloned() {
                    let mut fmap: HashMap<String, RefineVal> = HashMap::new();
                    let mut all_const = true;
                    for (fname, fexpr) in fields {
                        if let Some(v) = const_eval_int(fexpr) {
                            fmap.insert(fname.clone(), RefineVal::Int(v));
                        } else if let Expr::Literal(crate::ast::Literal::Str(s)) = fexpr {
                            fmap.insert(fname.clone(), RefineVal::Str(s.clone()));
                        } else {
                            all_const = false;
                            break;
                        }
                    }
                    if all_const
                        && self.eval_refinement_pred(&pred, &RefineVal::Struct(fmap)) == Some(false)
                    {
                        let file = self.file.clone();
                        let span = self.current_span;
                        self.errors.push(
                            CheckError::new(
                                E1209,
                                format!(
                                    "this `{name}` literal violates the struct refinement — its \
                                     constant fields do not satisfy the type's predicate"
                                ),
                            )
                            .node(node_path)
                            .at(&file, 0, 0)
                            .with_span(span)
                            .fix(format!(
                                "set the fields so `{name}`'s `where` predicate holds"
                            )),
                        );
                    }
                }
                // Duplicate field in the literal (`P { x: 1, x: 2 }`): last-wins
                // silently dropped the first value. Flag each repeat as E0406.
                {
                    let mut seen: std::collections::HashSet<&str> =
                        std::collections::HashSet::new();
                    for (fname, _) in fields {
                        if !seen.insert(fname.as_str()) {
                            let file = self.file.clone();
                            self.errors.push(
                                CheckError::new(
                                    E0406,
                                    format!("field `{fname}` is set more than once in this `{name}` literal"),
                                )
                                .node(node_path)
                                .at(&file, 0, 0)
                                .fix(format!("remove the duplicate `{fname}:` entry")),
                            );
                        }
                    }
                }
                // Enum-variant literal: `Enum::Variant`. Validate the variant
                // actually exists on the enum — `S::C` for a missing `C` was
                // silently accepted (built a bogus Value::Enum), then matching it
                // panicked at runtime ("no match arm matched"). Flag E0404.
                if let Some((enum_name, variant)) = name.split_once("::") {
                    if let Some(variants) = self.enum_variants.get(enum_name) {
                        if !variants.iter().any(|v| v == variant) {
                            let file = self.file.clone();
                            self.errors.push(
                                CheckError::new(
                                    E0404,
                                    format!("enum `{enum_name}` has no variant `{variant}`"),
                                )
                                .node(node_path)
                                .at(&file, 0, 0)
                                .fix(format!("`{enum_name}` variants: {}", variants.join(", "))),
                            );
                        }
                    }
                }
            }

            // ── Leaves ───────────────────────────────────────────────────────
            Expr::Ident(_)
            | Expr::Literal(_)
            | Expr::None
            | Expr::Array(_)
            | Expr::Tuple(_)
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
                        .fix(
                            "add `?` to propagate the error, or wrap the call in \
                              `match call() { Ok(v) => v, Err(e) => /* handle */ }`",
                        ),
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

    /// R13 E1803: a native `Handle` is opaque — it cannot be constructed,
    /// indexed, or used in arithmetic. Fires when an operand at an arithmetic /
    /// index / negation site resolves to a native handle carrier. This is what
    /// makes a handle UNFORGEABLE at the surface: there is no operation that
    /// turns an i64 into a handle or pulls an index out of one.
    fn check_handle_not_arithmetic(&mut self, ty: &Type, node_path: &str, verb: &str) {
        if crate::native::is_native_handle(ty) {
            let file = self.file.clone();
            self.errors.push(
                CheckError::new(
                    crate::error::E1803,
                    format!(
                        "`Handle` is opaque — it cannot be {verb}; a native handle is an \
                         unforgeable token, not a number"
                    ),
                )
                .node(node_path)
                .at(&file, 0, 0)
                .fix(
                    "pass the handle to the native function that consumes or borrows it; \
                     do not do arithmetic on it"
                        .to_string(),
                ),
            );
        }
    }

    /// R13: validate the arguments of a native `M::fn(...)` call against its
    /// FFI signature. Emits E1801 (a non-FFI-representable arg type) and E1802
    /// (a handle from a different module than the param expects). Arity is
    /// checked too (reusing the generic too-few/too-many message shape).
    fn check_native_call_args(
        &mut self,
        nf: &crate::native::NativeFn,
        args: &[Expr],
        node_path: &str,
        scope: &mut HashMap<String, Type>,
    ) {
        let file = self.file.clone();
        if args.len() != nf.params.len() {
            self.errors.push(
                CheckError::new(
                    crate::error::E1801,
                    format!(
                        "native fn `{}` takes {} argument(s), but {} were supplied",
                        nf.name,
                        nf.params.len(),
                        args.len()
                    ),
                )
                .node(node_path)
                .at(&file, 0, 0),
            );
            return;
        }
        for (i, arg) in args.iter().enumerate() {
            let apath = format!("{node_path}.arg_{i}");
            let arg_ty = self.resolve_expr_type(arg, &apath, scope);
            // Unknown/Var: inference hasn't resolved it — let other passes own it.
            if matches!(arg_ty, Type::Unknown | Type::Var(_)) {
                continue;
            }
            // E1801: the arg type must be FFI-representable. A user struct,
            // Option, Result, closure, generic, tuple, etc. is refused HERE at
            // check time — it never reaches codegen (mirrors E0910).
            if !crate::native::is_ffi_repr(&arg_ty) {
                self.errors.push(
                    CheckError::new(
                        crate::error::E1801,
                        format!(
                            "type `{}` is not FFI-representable at the native boundary \
                             (allowed: scalars, str, [scalar], Handle)",
                            arg_ty.display()
                        ),
                    )
                    .node(&apath)
                    .at(&file, 0, 0)
                    .found(arg_ty.display())
                    .fix(
                        "pass only scalars, str, a [scalar] slice, or a native Handle \
                         across the FFI boundary"
                            .to_string(),
                    ),
                );
                continue;
            }
            // E1802: a handle arg must be the same module+name the param expects
            // — handles do not cross modules.
            let (expected_ty, _mode) = &nf.params[i];
            let expected = expected_ty.to_type();
            if crate::native::is_native_handle(&expected)
                && crate::native::is_native_handle(&arg_ty)
            {
                if let (Type::Deferred(ek), Type::Deferred(ak)) = (&expected, &arg_ty) {
                    if ek != ak {
                        // Extract a readable B::H / A::H label.
                        let exp_lbl = crate::native::parse_handle_key(ek)
                            .map(|(m, n, _)| format!("{m}::{n}"))
                            .unwrap_or_else(|| ek.clone());
                        let got_lbl = crate::native::parse_handle_key(ak)
                            .map(|(m, n, _)| format!("{m}::{n}"))
                            .unwrap_or_else(|| ak.clone());
                        self.errors.push(
                            CheckError::new(
                                crate::error::E1802,
                                format!(
                                    "expected handle `{exp_lbl}`, found `{got_lbl}` — handles \
                                     do not cross modules"
                                ),
                            )
                            .node(&apath)
                            .at(&file, 0, 0)
                            .expected(exp_lbl)
                            .found(got_lbl),
                        );
                    }
                }
            }
        }
    }

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
                    format!("arithmetic operand has non-numeric type {}", ty.display()),
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
                let params: Vec<String> = sig.params.iter().map(|p| p.display()).collect();
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
        // Bindings for the type parameters a builtin signature names more than
        // once (`arr_push([T], T)`, `arr_contains`, `arr_index_of`,
        // `arr_concat`): tp name → (arg index that bound it, concrete type).
        let mut tp_bindings: HashMap<String, (usize, Type)> = HashMap::new();
        for (i, (arg, param_ty)) in args.iter().zip(sig.params.iter()).enumerate() {
            let arg_path = format!("{node_path}.arg_{i}");
            let arg_ty = self.resolve_expr_type(arg, &arg_path, scope);

            // A generic slot accepts ANY type — but the same `T` in two slots
            // must be the SAME type, or "generic" would just mean "untyped".
            // `arr_push([1, 2], "str")` binds T=i64 then T=str: refuse.
            let mut bindings: Vec<(String, Type)> = Vec::new();
            collect_builtin_type_param_bindings(param_ty, &arg_ty, &mut bindings);
            let mut conflicted = false;
            for (tp, bound) in bindings {
                match tp_bindings.get(&tp) {
                    Option::Some((first_i, first_ty)) => {
                        if *first_ty != bound
                            && !is_integer_widening(&bound, first_ty)
                            && !is_integer_widening(first_ty, &bound)
                        {
                            let file = self.file.clone();
                            let span = self.current_span;
                            let first_disp = first_ty.display();
                            let bound_disp = bound.display();
                            let first_i = *first_i;
                            self.errors.push(
                                CheckError::new(
                                    E0306,
                                    format!(
                                        "argument {i} of `{name}` has the wrong type — the \
                                         element type `{tp}` was already fixed to \
                                         `{first_disp}` by argument {first_i}"
                                    ),
                                )
                                .node(&arg_path)
                                .at(&file, 0, 0)
                                .with_span(span)
                                .expected(first_disp.clone())
                                .found(bound_disp)
                                .fix(format!(
                                    "`{name}` is generic but not heterogeneous — every `{tp}` \
                                     slot must be the same type; pass a `{first_disp}` here, or \
                                     make argument {first_i} match"
                                )),
                            );
                            conflicted = true;
                        }
                    }
                    Option::None => {
                        tp_bindings.insert(tp, (i, bound));
                    }
                }
            }
            if conflicted {
                continue;
            }

            // A *concrete wrapper* arg (`()`, `Option<_>`, `Result<_,_>`) can
            // never satisfy a *deferred opaque* parameter (`Dict`, `Uncertain`,
            // `Temporal`, `Goal`). This must run BEFORE the deferred/unknown skip
            // below, which fires on `param_ty.is_deferred()` REGARDLESS of the
            // arg — so it would swallow a provably-wrong arg.
            //
            // The trap: builtins like `dict_set`/`dict_inc` mutate in place and
            // return `()`, `dict_get` returns `Option<V>`, `dict_to_str` returns
            // `Result<str,str>` — and their first param is the deferred `Dict`.
            // Feeding any of those back into a `Dict` slot —
            //     dict_set(dict_set(d,…),…)   // () into Dict
            //     dict_set(dict_get(d,k),…)   // Option into Dict
            //     dict_len(dict_to_str(d))    // Result into Dict
            // — was waved through by the deferred-skip and surfaced only as an
            // interpreter panic ("expected dict, got Option") or a codegen E0701,
            // never as a clean compile-time error.
            //
            // These wrapper variants are fully concrete and are NEVER what a
            // deferred opaque type unifies to (`Type::Deferred` only ever wraps
            // Dict/Uncertain/Temporal/Goal — checked: it never wraps
            // Option/Result/Unit). A generic *value* slot (`dict_set`'s `v: T`)
            // is `Type::TypeParam`, NOT deferred — so legitimately storing an
            // `Option` as a dict value is unaffected.
            // M4: a NAMED function passed where a closure is expected.
            //
            // `arr_map([1,2,3], double)` passed `axon check` and then PANICKED at
            // run with "undefined identifier `double`" — a check/run soundness
            // divergence, and the interpreter is this project's reference oracle,
            // so the checker accepting it is the bug. Passing a named fn to a
            // higher-order builtin is the first thing a model writes.
            //
            // Refused rather than supported: making the interpreter resolve
            // fn-names-as-values would oblige native codegen to match or create
            // an interp/native divergence (invariant I-2), which is a language
            // feature, not a fix. The diagnostic names the working form instead,
            // so the reader is one edit away rather than stuck.
            // NOTE the type test: `parse_type_str` (infer.rs:167) has no `fn(`
            // arm, so a builtin's `fn(T) -> U` parameter lands as
            // `Type::Deferred("fn(T) -> U")`, not `Type::Fn`. Matching on
            // `Type::Fn` here compiled and silently never fired — the same
            // deferred-swallows-the-check class this function already documents
            // for `Dict`.
            let param_is_fn = matches!(param_ty, Type::Fn(..))
                || matches!(param_ty, Type::Deferred(n) if n.starts_with("fn("));
            if param_is_fn {
                if let Expr::Ident(callee) = arg {
                    let is_local = scope.contains_key(callee);
                    if !is_local && self.fn_sigs.contains_key(callee) {
                        let file = self.file.clone();
                        self.errors.push(
                            CheckError::new(
                                E0306,
                                format!(
                                    "argument {i} of `{name}` is the function \
                                     `{callee}` passed by name, which Axon cannot \
                                     evaluate as a value"
                                ),
                            )
                            .node(&arg_path)
                            .at(&file, 0, 0)
                            .fix(format!("wrap it in a lambda — `|x| {callee}(x)`")),
                        );
                        continue;
                    }
                }
            }

            let arg_is_concrete_wrapper =
                matches!(arg_ty, Type::Unit | Type::Option(_) | Type::Result(_, _));
            // Only an *opaque* deferred param (Dict/Uncertain/Temporal/Goal)
            // rejects these wrappers. A bare generic type-param also resolves to
            // `Type::Deferred` (e.g. `dict_set`'s value `v: T` → `Deferred("T")`),
            // but a `T` slot legitimately accepts ANY value — including an
            // `Option` stored as a dict value. Distinguish the two by the
            // deferred name: an opaque type starts with a DEFERRED_PREFIXES entry;
            // a type-param (`T`, `K`, `V`, `E`) does not.
            let param_is_opaque_deferred = matches!(param_ty, Type::Deferred(n)
                if DEFERRED_PREFIXES.iter().any(|p| n.starts_with(p)));
            if arg_is_concrete_wrapper && param_is_opaque_deferred {
                let file = self.file.clone();
                let span = self.current_span;
                // Friendlier `found` label: the inner of these synthesized
                // wrappers is `Unknown` (the payload is genuinely unresolved),
                // which would render as `Option<<unknown>>`. Show `Option<_>` /
                // `Result<_,_>` / `()` instead.
                let found_disp = match &arg_ty {
                    Type::Unit => "()".to_string(),
                    Type::Option(_) => "Option<_>".to_string(),
                    _ => "Result<_, _>".to_string(),
                };
                let hint = match &arg_ty {
                    Type::Unit => format!(
                        "this argument is `()` — the value of a `()`-returning call (e.g. \
                         `dict_set`/`dict_inc` mutate in place and return `()`); bind the \
                         original `{}` to a variable and pass that instead of nesting the call",
                        param_ty.display()
                    ),
                    Type::Option(_) => format!(
                        "this argument is an `{found_disp}` (e.g. from `dict_get`); unwrap it \
                         with `match`/`unwrap_or` to obtain a `{}` before passing it",
                        param_ty.display()
                    ),
                    _ => format!(
                        "this argument is a `{found_disp}` (e.g. from `dict_to_str`); unwrap it \
                         with `match`/`?` to obtain a `{}` before passing it",
                        param_ty.display()
                    ),
                };
                self.errors.push(
                    CheckError::new(
                        E0306,
                        format!("argument {i} of `{name}` has the wrong type"),
                    )
                    .node(&arg_path)
                    .at(&file, 0, 0)
                    .with_span(span)
                    .expected(param_ty.display())
                    .found(found_disp)
                    .fix(hint),
                );
                continue;
            }

            // R19: a literal int arg coerces to a fixed-width/unsigned param —
            // infer owns the coercion + E1900 range-check, so don't double-report
            // E0306 here. Sound: only *literals* coerce; a non-literal int →
            // narrower/unsigned still mismatches, and unsigned arithmetic stays
            // rejected until Slice B (I-9, no i64-backed half-measure).
            if matches!(arg, Expr::Literal(crate::ast::Literal::Int(_)))
                && is_int_width(param_ty)
                && is_int_width(&arg_ty)
                && *param_ty != arg_ty
            {
                continue;
            }

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
                // `len` accepts a slice/array as well as its declared `str` param
                // (the interpreter handles both); mirrors the infer special-case.
                || (name == "len" && matches!(arg_ty, Type::Slice(_)))
                // `to_str` is polymorphic over scalars (i64/i32/.../f64/bool);
                // the interpreter dispatches on the runtime value (BUG_HUNT #29).
                // Mirrors the infer special-case. A non-scalar arg still flows to
                // the declared `i64` param and errors below.
                || (name == "to_str" && arg_ty.is_scalar())
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
            let dyn_coercion_ok = if let Type::DynTrait(trait_name) = param_ty {
                let concrete_name = match &arg_ty {
                    Type::Struct(n) | Type::Enum(n) => Some(n.as_str()),
                    _ => None,
                };
                concrete_name
                    .map(|n| {
                        self.impl_table
                            .get(n)
                            .map(|set| set.contains(trait_name.as_str()))
                            .unwrap_or(false)
                    })
                    .unwrap_or(false)
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
                } else if matches!(param_ty, Type::Str)
                    && matches!(arg_ty, Type::I64 | Type::I32 | Type::I16 | Type::I8)
                {
                    // A `str` parameter handed an integer. The generic advice below
                    // ("cast with `as str`") is ACTIVELY WRONG here: there is no
                    // int->str cast, and `to_str` yields the number's DIGITS, not a
                    // character. Measured against the RLM harness this is the blocker
                    // on 3 of 8 tasks — the model writes
                    // `str_contains(vowels, char_at(s, i))` or `char_at(s, i) == " "`,
                    // because per-character work is the obvious approach and
                    // `char_at` returns a BYTE VALUE. Pointing it at `as str` burns
                    // the whole repair round, so name the idiom that works.
                    //
                    // `char_at` is named unconditionally rather than only when the
                    // argument is literally a `char_at(...)` call: the usual shape
                    // binds it first (`let c = char_at(s, i)`) and passes the
                    // IDENTIFIER, which this check cannot trace back. An int where a
                    // str is expected is overwhelmingly char_at-derived in string
                    // code, so the mention earns its place even when it is not.
                    format!(
                        "expected `str`, found `{found_disp}` — there is no `as str` cast, and \
                         `to_str(x)` gives a number's DIGITS. If this value came from \
                         `char_at` (which returns a BYTE VALUE), use `str_slice(s, i, i + 1)` \
                         for a one-character `str` instead"
                    )
                } else {
                    format!(
                        "expected `{expected_disp}`, found `{found_disp}` — \
                         change the argument's type or cast with `as {expected_disp}` if compatible"
                    )
                };
                self.errors.push(
                    CheckError::new(
                        E0306,
                        // The expected/found pair is carried by the structured
                        // `.expected()/.found()` fields below; every renderer
                        // (CLI JSON, `display()`, LSP) re-appends it, so keep it
                        // out of the message itself to avoid printing it twice
                        // (cf. E0307). The `i` index + `name` stay in the message.
                        format!("argument {i} of `{name}` has the wrong type"),
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
                    Type::I8 => "i8".into(),
                    Type::U64 => "u64".into(),
                    Type::U32 => "u32".into(),
                    Type::U16 => "u16".into(),
                    Type::U8 => "u8".into(),
                    Type::F64 => "f64".into(),
                    Type::F32 => "f32".into(),
                    Type::Bool => "bool".into(),
                    Type::Str => "str".into(),
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

        // Unreachable-arm detection (W0004): a later arm whose head constructor
        // exactly duplicates an earlier one is dead code (`S::A => …  S::A => …`,
        // `None => …  None => …`, `0 => …  0 => …`). Only flag heads we can match
        // CONCLUSIVELY equal — skip bind/wildcard/deep-subpattern arms (a guard or
        // a sub-pattern difference can make a repeat reachable). The first
        // occurrence wins; warn on each subsequent duplicate.
        {
            let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
            let mut catch_all_seen = false;
            for arm in arms {
                let file = self.file.clone();
                // Any arm AFTER an unguarded catch-all (`_` or a bare binding) is
                // unreachable — the catch-all already matched everything.
                if catch_all_seen {
                    self.errors.push(
                        CheckError::warning(
                            crate::error::W0004,
                            "unreachable match arm: an earlier `_`/binding arm already covers every value".to_string(),
                        )
                        .node(node_path)
                        .at(&file, 0, 0)
                        .fix("remove this arm, or move it above the catch-all".to_string()),
                    );
                    continue;
                }
                // A guard makes even an identical head potentially reachable, and
                // a GUARDED catch-all does not actually cover everything.
                if arm.guard.is_some() {
                    continue;
                }
                if matches!(arm.pattern, Pattern::Wildcard | Pattern::Ident(_)) {
                    catch_all_seen = true;
                    continue;
                }
                if let Some(key) = conclusive_pattern_key(&arm.pattern) {
                    if !seen.insert(key.clone()) {
                        self.errors.push(
                            CheckError::warning(
                                crate::error::W0004,
                                format!("unreachable match arm: `{key}` is already covered by an earlier arm"),
                            )
                            .node(node_path)
                            .at(&file, 0, 0)
                            .fix("remove the duplicate arm, or change its pattern".to_string()),
                        );
                    }
                }
            }
        }

        // Literal-pattern type check (E0405): a literal pattern whose type can't
        // match the subject is always-dead (`match n /*i64*/ { "x" => … }`,
        // `match b /*bool*/ { 5 => … }`). Silently fell through to a catch-all
        // before. Only fires when the subject is a concrete scalar and the
        // literal's type definitively differs (i64/i32 are mutually compatible).
        if let Some(subj) = scalar_kind(subject_ty) {
            for arm in arms {
                if let Pattern::Literal(lit) = &arm.pattern {
                    let lit_kind = literal_scalar_kind(lit);
                    if lit_kind != subj {
                        let file = self.file.clone();
                        self.errors.push(
                            CheckError::new(
                                E0405,
                                format!(
                                    "this match is on `{}`, but the pattern is a `{lit_kind}` literal — it can never match",
                                    subject_ty.display()
                                ),
                            )
                            .node(node_path)
                            .at(&file, 0, 0)
                            .expected(subj.to_string())
                            .found(lit_kind.to_string())
                            .fix(format!("use a `{subj}` literal pattern, or match on a `{lit_kind}` value")),
                        );
                    }
                }
            }
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

    fn check_return_type_match(&mut self, val_ty: &Type, node_path: &str, _val_path: &str) {
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
                (Type::Unit, _) => "the function returns `()`; remove the trailing expression \
                     or end the block with `;`"
                    .to_string(),
                _ => format!(
                    "the function declares `-> {expected}`, but the body produces `{found}` — \
                     adjust the final expression (or change the declared return type)"
                ),
            };
            self.errors.push(
                CheckError::new(
                    E0307,
                    // The expected/found pair is carried by the structured
                    // `.expected()/.found()` fields below; every renderer
                    // (CLI JSON, `display()`, LSP) re-appends it, so keep it
                    // out of the message itself to avoid printing it twice.
                    "return type mismatch".to_string(),
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
    // R6 transitive taint — which fn params reach an exfiltration sink
    // ─────────────────────────────────────────────────────────────────────────

    /// Populate `self.exfiltrating_params` to a fixpoint. A parameter index of a
    /// function is "exfiltrating" when that parameter's value flows — as a direct
    /// identifier argument — into an exfiltration sink: a builtin sink
    /// (`ai_complete`/`ai_extract*`/`write_file`/`exec`), or another user fn at one
    /// of ITS already-known exfiltrating positions. Iterating to a fixpoint
    /// propagates taint through chains of helpers.
    fn compute_exfiltrating_params(&mut self, program: &Program) {
        // Collect the (name, param-names, body) of every user fn once — free fns
        // AND impl-block methods (keyed by the mangled `{Type}__{method}`, matching
        // infer's collect_sigs, so a MethodCall site can look the method up). Was
        // free-fns-only, so sensitive data laundered through a METHOD that forwards
        // an arg to a sink escaped E1206 (the MethodCall-vs-Call gap class).
        let ast_type_simple_name = |ty: &AxonType| -> String {
            match ty {
                AxonType::Named(n) => n.clone(),
                AxonType::Generic { base, .. } => base.clone(),
                _ => "Unknown".into(),
            }
        };
        let mut fns: Vec<(String, Vec<String>, &Expr)> = Vec::new();
        for item in &program.items {
            match item {
                Item::FnDef(f) => fns.push((
                    f.name.clone(),
                    f.params.iter().map(|p| p.name.clone()).collect(),
                    &f.body,
                )),
                Item::ImplBlock(blk) => {
                    let ty = ast_type_simple_name(&blk.for_type);
                    for m in &blk.methods {
                        fns.push((
                            format!("{ty}__{}", m.name),
                            m.params.iter().map(|p| p.name.clone()).collect(),
                            &m.body,
                        ));
                    }
                }
                _ => {}
            }
        }

        let mut changed = true;
        // Bounded by the number of (fn, param) pairs; the fixpoint converges
        // because the exfiltrating set only grows. The outer guard is a safety
        // cap against any pathological case.
        let max_rounds = fns.len() + 2;
        let mut rounds = 0;
        while changed && rounds <= max_rounds {
            changed = false;
            rounds += 1;
            for (fname, params, body) in &fns {
                for (idx, pname) in params.iter().enumerate() {
                    // (1) Param flows to an exfiltration sink.
                    if !self
                        .exfiltrating_params
                        .get(fname)
                        .is_some_and(|s| s.contains(&idx))
                        && self.param_reaches_sink(pname, body)
                    {
                        self.exfiltrating_params
                            .entry(fname.clone())
                            .or_default()
                            .insert(idx);
                        changed = true;
                    }
                    // (2) Param's sensitivity flows to the RETURN value.
                    if !self
                        .taint_returning_params
                        .get(fname)
                        .is_some_and(|s| s.contains(&idx))
                        && self.param_reaches_return(pname, body)
                    {
                        self.taint_returning_params
                            .entry(fname.clone())
                            .or_default()
                            .insert(idx);
                        changed = true;
                    }
                }
            }
        }
    }

    /// True when the parameter `pname` flows to the function's RETURN value: it
    /// appears (as the ident, a field of it, an index, an interpolation, or the
    /// result of calling a taint-returning fn with it) in a tail/return position.
    fn param_reaches_return(&self, pname: &str, body: &Expr) -> bool {
        self.expr_carries_param_taint(pname, self.tail_exprs(body).as_slice())
    }

    /// The set of expressions in RETURN position of a body: the block's trailing
    /// expression (recursively through nested blocks / if / match arms) plus every
    /// explicit `return <e>`.
    fn tail_exprs<'e>(&self, body: &'e Expr) -> Vec<&'e Expr> {
        let mut out = Vec::new();
        collect_return_exprs(body, &mut out);
        out
    }

    /// True if any of `exprs` carries the taint of param `pname` — directly
    /// (ident / field / index / fmtstr), or as the result of a call to a
    /// taint-returning fn whose tainted-arg position carries `pname`.
    fn expr_carries_param_taint(&self, pname: &str, exprs: &[&Expr]) -> bool {
        exprs.iter().any(|e| {
            if arg_carries_ident(e, pname) {
                return true;
            }
            if let Expr::Call { callee, args, .. } = e {
                if let Expr::Ident(cn) = callee.as_ref() {
                    if let Some(positions) = self.taint_returning_params.get(cn) {
                        for (i, arg) in args.iter().enumerate() {
                            if positions.contains(&i) && arg_carries_ident(arg, pname) {
                                return true;
                            }
                        }
                    }
                }
            }
            false
        })
    }

    /// True when the identifier `pname` flows as a direct argument into an
    /// exfiltration sink anywhere in `body` (a builtin sink, or a user fn at one
    /// of its currently-known exfiltrating positions). Reads
    /// `self.exfiltrating_params` (the partial fixpoint).
    fn param_reaches_sink(&self, pname: &str, body: &Expr) -> bool {
        let mut found = false;
        self.walk_for_sink_flow(pname, body, &mut found);
        found
    }

    fn walk_for_sink_flow(&self, pname: &str, e: &Expr, found: &mut bool) {
        if *found {
            return;
        }
        if let Expr::Call { callee, args, .. } = e {
            if let Expr::Ident(callee_name) = callee.as_ref() {
                for (i, arg) in args.iter().enumerate() {
                    // Does this arg carry the parameter `pname`? Either the bare
                    // identifier, or a field access on it (`p.email`), or an
                    // interpolation that mentions it.
                    if arg_carries_ident(arg, pname) {
                        let is_builtin_sink = exfiltration_sink_kind(callee_name).is_some();
                        let is_user_sink = self
                            .exfiltrating_params
                            .get(callee_name)
                            .is_some_and(|s| s.contains(&i));
                        if is_builtin_sink || is_user_sink {
                            *found = true;
                            return;
                        }
                    }
                }
            }
        }
        // Recurse into every sub-expression.
        each_subexpr(e, &mut |sub| self.walk_for_sink_flow(pname, sub, found));
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
                if !is_known_type_name(name, &self.struct_fields, &known_enums)
                    && !self.refinement_base.contains_key(name)
                {
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
            AxonType::Chan(inner)
            | AxonType::Slice(inner)
            | AxonType::Ref(inner)
            | AxonType::RawPtr(inner) => {
                self.check_axon_type(inner, &format!("{node_path}.inner"));
            }
            AxonType::Generic { base, args } => {
                // Validate the base name (deferred prefixes are always OK).
                self.check_axon_type(&AxonType::Named(base.clone()), &format!("{node_path}.base"));
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

        // Layer-1 ASI: Uncertain<T> / Temporal<T> have a fixed virtual field set.
        match recv_ty {
            Type::Uncertain(_) => {
                if !matches!(field, "value" | "confidence" | "source_tag") {
                    let file = self.file.clone();
                    self.errors.push(
                        CheckError::new(E0401, format!("Uncertain<T> has no field '{field}'"))
                            .node(node_path)
                            .at(&file, 0, 0)
                            // No `.found(field)`: the driver appends ", found {found}"
                            // and the field name isn't a type — it would render the
                            // nonsensical "has no field 'x', found x" (cf. 2bcee30).
                            // The valid field set rides `fix` (→ help).
                            .fix("Uncertain<T> fields: value, confidence, source_tag"),
                    );
                }
                return;
            }
            Type::Temporal(_) => {
                if !matches!(
                    field,
                    "value" | "confidence" | "horizon_ms" | "decay" | "valid_until_ms"
                ) {
                    let file = self.file.clone();
                    self.errors.push(
                        CheckError::new(
                            E0401,
                            format!("Temporal<T> has no field '{field}'"),
                        )
                        .node(node_path)
                        .at(&file, 0, 0)
                        // No `.found(field)` — see the Uncertain<T> note above.
                        .fix("Temporal<T> fields: value, confidence, horizon_ms, decay, valid_until_ms"),
                    );
                }
                return;
            }
            _ => {}
        }

        // Tuple field access: numeric index, in-range.
        if let Type::Tuple(elems) = recv_ty {
            match field.parse::<usize>() {
                Ok(i) if i < elems.len() => return,
                Ok(i) => {
                    let file = self.file.clone();
                    self.errors.push(
                        CheckError::new(
                            E0401,
                            format!("tuple index {i} out of bounds (length {})", elems.len()),
                        )
                        .node(node_path)
                        .at(&file, 0, 0),
                        // No `.found(field)`: the index is already in the message;
                        // the driver's ", found {found}" suffix would echo it as a
                        // bogus "type" (cf. 2bcee30).
                    );
                    return;
                }
                Err(_) => {
                    let file = self.file.clone();
                    self.errors.push(
                        CheckError::new(
                            E0401,
                            format!("tuple field must be a numeric index, got '{field}'"),
                        )
                        .node(node_path)
                        .at(&file, 0, 0),
                        // No `.found(field)` — see the tuple-OOB note above.
                    );
                    return;
                }
            }
        }

        match recv_ty {
            Type::Struct(struct_name) => {
                match self.struct_fields.get(struct_name).cloned() {
                    Option::Some(fields) => {
                        if !fields.iter().any(|(n, _)| n == field) {
                            let field_names: Vec<String> =
                                fields.iter().map(|(n, _)| n.clone()).collect();
                            let file = self.file.clone();
                            self.errors.push(
                                CheckError::new(
                                    E0401,
                                    format!("struct '{}' has no field '{field}'", struct_name),
                                )
                                .node(node_path)
                                .at(&file, 0, 0)
                                // No `.found(field)`: the driver appends ", found
                                // {found}" to the message, which for a field-
                                // existence error reads as the nonsensical
                                // "has no field 'z', found z". The known-fields
                                // list already rides in `fix` (→ help).
                                .fix(format!(
                                    "'{struct_name}' fields: {}",
                                    field_names.join(", ")
                                )),
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
                    CheckError::new(E0401, format!("{} has no field '{field}'", other.display()))
                        .node(node_path)
                        .at(&file, 0, 0),
                    // No `.found(field)`: the field name is already named in the
                    // message, and it's not a type — the driver's ", found
                    // {found}" suffix would render "i64 has no field 'foo', found
                    // foo" (the same wart 2bcee30 fixed for the struct arm).
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
    /// PRD §4: does `arg` carry sensitive data into a sink? Returns
    /// `Some((source_name, category))` when the arg's resolved type is a
    /// `@[sensitive]` struct, OR when the arg is a field access whose receiver
    /// is a sensitive struct (`u.email`). The field case names the struct as
    /// the source (a field of a sensitive type inherits its sensitivity — the
    /// "can't exfiltrate sensitive fields" rule). Returns `None` otherwise.
    fn sensitive_flow_in(
        &self,
        arg: &Expr,
        apath: &str,
        scope: &HashMap<String, Type>,
    ) -> Option<(String, String)> {
        // (a0) The arg is a bare local that was bound to a sensitive value earlier
        // in this function (`let e = u.email; sink(e)`). Its static type is a plain
        // scalar, but its provenance is tainted (tracked in `sensitive_locals`).
        if let Expr::Ident(n) = arg {
            if let Some(src) = self.sensitive_locals.get(n) {
                return Some(src.clone());
            }
        }
        // (a0b) A field/element extracted from a tainted NON-struct local — a
        // container that bundled a sensitive value (`let w = Wrapper{data:u.email}
        // ...}; sink(w.data)`; `let t = (u.email, _); sink(t.0)`). The whole local
        // is conservatively tainted. Guarded so a sensitive STRUCT local falls
        // through to case (b), which produces the precise `Struct.field` source.
        if let Expr::FieldAccess { receiver, .. } | Expr::Index { receiver, .. } = arg {
            // Walk to the root identifier.
            let mut root = receiver.as_ref();
            while let Expr::FieldAccess { receiver: r, .. } | Expr::Index { receiver: r, .. } = root
            {
                root = r.as_ref();
            }
            if let Expr::Ident(n) = root {
                let receiver_is_sensitive_struct = matches!(
                    self.resolve_expr_type(receiver, &format!("{apath}.receiver"), scope),
                    Type::Struct(ref s) if self.sensitive_types.contains_key(s)
                );
                if !receiver_is_sensitive_struct {
                    if let Some(src) = self.sensitive_locals.get(n) {
                        return Some(src.clone());
                    }
                }
            }
        }
        // (a1) The arg is a call to a TAINT-RETURNING fn whose tainting parameter
        // is itself fed a sensitive value (`get_email(u)` where get_email returns
        // `u.email` and `u` is sensitive). The result inherits the sensitivity.
        if let Expr::Call { callee, args, .. } = arg {
            if let Expr::Ident(cn) = callee.as_ref() {
                if let Some(positions) = self.taint_returning_params.get(cn) {
                    for (i, inner) in args.iter().enumerate() {
                        if positions.contains(&i) {
                            let ipath = format!("{apath}.arg_{i}");
                            if let Some(src) = self.sensitive_flow_in(inner, &ipath, scope) {
                                return Some(src);
                            }
                        }
                    }
                }
            }
        }
        // (a) The whole value is a sensitive struct.
        if let Type::Struct(sname) = self.resolve_expr_type(arg, apath, scope) {
            if let Some(cat) = self.sensitive_types.get(&sname) {
                return Some((sname, cat.clone()));
            }
        }
        // (b) A field access `<receiver>.field`.
        if let Expr::FieldAccess { receiver, field } = arg {
            let rpath = format!("{apath}.receiver");
            if let Type::Struct(rstruct) = self.resolve_expr_type(receiver, &rpath, scope) {
                // (b1) The receiver itself is a sensitive struct — reading any of
                // its fields exfiltrates sensitive data (PRD "can't exfiltrate
                // sensitive fields").
                if let Some(cat) = self.sensitive_types.get(&rstruct) {
                    return Some((format!("{rstruct}.{field}"), cat.clone()));
                }
                // (b2) The FIELD's declared type is itself a sensitive struct
                // (`w.user` where Wrapper.user: User and User is @[sensitive]).
                // resolve_expr_type doesn't resolve nested field types, so look
                // the field's type up directly in the struct field map.
                if let Some(fields) = self.struct_fields.get(&rstruct) {
                    if let Some((_, Type::Struct(fstruct))) =
                        fields.iter().find(|(n, _)| n == field)
                    {
                        if let Some(cat) = self.sensitive_types.get(fstruct) {
                            return Some((fstruct.clone(), cat.clone()));
                        }
                    }
                }
            }
        }
        // (c) A composite literal that BUNDLES a sensitive value — an array
        // (`[u.name]`) or a struct literal (`Wrapper { user: u }`). Recurse into
        // the elements/field values so wrapping a sensitive value in a container
        // doesn't launder it past the sink.
        match arg {
            Expr::Array(elems) => {
                for (i, e) in elems.iter().enumerate() {
                    if let Some(found) =
                        self.sensitive_flow_in(e, &format!("{apath}.elem_{i}"), scope)
                    {
                        return Some(found);
                    }
                }
            }
            Expr::StructLit { fields, .. } => {
                for (fname, e) in fields {
                    if let Some(found) =
                        self.sensitive_flow_in(e, &format!("{apath}.{fname}"), scope)
                    {
                        return Some(found);
                    }
                }
            }
            Expr::Tuple(elems) => {
                for (i, e) in elems.iter().enumerate() {
                    if let Some(found) =
                        self.sensitive_flow_in(e, &format!("{apath}.elem_{i}"), scope)
                    {
                        return Some(found);
                    }
                }
            }
            // (d) A BUILTIN call that DERIVES its result from a sensitive arg —
            // `str_to_upper(u.email)`, `str_concat(u.email, x)`, `str_len(u.email)`.
            // Every builtin's output is a function of its inputs (none discard +
            // re-synthesise data), so the result still carries the secret; recurse
            // into its args. User fns are NOT blanket-recursed here — their precise
            // flow is (a1) above (taint only when the fn returns its tainted arg),
            // so a fn that drops its arg doesn't cause a false positive.
            Expr::Call { callee, args, .. } => {
                if let Expr::Ident(cn) = callee.as_ref() {
                    if crate::builtins::is_known_builtin(cn) {
                        for (i, e) in args.iter().enumerate() {
                            if let Some(found) =
                                self.sensitive_flow_in(e, &format!("{apath}.arg_{i}"), scope)
                            {
                                return Some(found);
                            }
                        }
                    }
                }
            }
            // (e) String interpolation — `"addr: {u.email}"` embeds the secret in
            // the result text. Recurse into each interpolated sub-expression. (The
            // direct-sink form is caught at the sink; this closes the let-bound
            // launder `let e = "{u.email}"; sink(e)`.)
            Expr::FmtStr { parts } => {
                for (i, p) in parts.iter().enumerate() {
                    if let crate::ast::FmtPart::Expr(e) = p {
                        if let Some(found) =
                            self.sensitive_flow_in(e, &format!("{apath}.part_{i}"), scope)
                        {
                            return Some(found);
                        }
                    }
                }
            }
            // (f) Arithmetic / comparison / logical ops over a sensitive operand —
            // the result is derived from (and leaks information about) the secret.
            Expr::BinOp { left, right, .. } => {
                if let Some(found) = self.sensitive_flow_in(left, &format!("{apath}.left"), scope) {
                    return Some(found);
                }
                if let Some(found) = self.sensitive_flow_in(right, &format!("{apath}.right"), scope)
                {
                    return Some(found);
                }
            }
            Expr::UnaryOp { operand, .. } => {
                if let Some(found) =
                    self.sensitive_flow_in(operand, &format!("{apath}.operand"), scope)
                {
                    return Some(found);
                }
            }
            // (g) Value-producing control flow — `let e = if c { u.email } else { x }`,
            // `let e = match k { _ => u.email }`. The result is one of the branch /
            // arm bodies, so a sensitive value in any of them flows out. (Residual:
            // a pattern that DESTRUCTURES a sensitive subject — `match u { User{email}
            // => email }` — binds the field to a fresh name not in sensitive_locals;
            // and result-depends-on-sensitive-subject leakage is not modelled. Both
            // need pattern-binding / control-dependence taint — documented limits.)
            Expr::If { then, else_, .. } => {
                if let Some(found) = self.sensitive_flow_in(then, &format!("{apath}.then"), scope) {
                    return Some(found);
                }
                if let Some(e) = else_ {
                    if let Some(found) = self.sensitive_flow_in(e, &format!("{apath}.else"), scope)
                    {
                        return Some(found);
                    }
                }
            }
            Expr::Match { arms, .. } => {
                for (i, arm) in arms.iter().enumerate() {
                    if let Some(found) =
                        self.sensitive_flow_in(&arm.body, &format!("{apath}.arm_{i}"), scope)
                    {
                        return Some(found);
                    }
                }
            }
            // (h) A block — the value is its tail expression.
            Expr::Block(stmts) => {
                if let Some(tail) = stmts.last() {
                    if let Some(found) =
                        self.sensitive_flow_in(&tail.expr, &format!("{apath}.tail"), scope)
                    {
                        return Some(found);
                    }
                }
            }
            _ => {}
        }
        None
    }

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
                crate::ast::Literal::Decimal(_) => Type::Decimal,
            },
            Expr::None => Type::Option(Box::new(Type::Unknown)),
            Expr::Some(inner) => {
                let inner_ty = self.resolve_expr_type(inner, &format!("{node_path}.inner"), scope);
                Type::Option(Box::new(inner_ty))
            }
            Expr::Ok(inner) => {
                let inner_ty = self.resolve_expr_type(inner, &format!("{node_path}.inner"), scope);
                Type::Result(Box::new(inner_ty), Box::new(Type::Unknown))
            }
            Expr::Err(inner) => {
                let inner_ty = self.resolve_expr_type(inner, &format!("{node_path}.inner"), scope);
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
                // R13: a native `M::fn(...)` call resolves to its FFI return type
                // (the StructLit-callee `::`-name resolves in the registry).
                if let Expr::StructLit { name, fields } = callee.as_ref() {
                    if fields.is_empty() {
                        if let Option::Some((_m, nf)) = crate::native::resolve_call(name) {
                            return nf.ret.to_type();
                        }
                    }
                }
                if let Expr::Ident(name) = callee.as_ref() {
                    if let Option::Some(sig) = self.fn_sigs.get(name.as_str()) {
                        if type_contains_unresolved(&sig.ret) {
                            // Collapsing the whole return to `Unknown` loses the
                            // OUTER wrapper shape — and the arg-type loop's
                            // `arg_ty == Unknown` skip then swallows it. For a
                            // builtin whose return is `Option<T>`/`Result<T,E>`
                            // with only the INNER type unresolved (e.g.
                            // `dict_get -> Option<T>`, `dict_to_str ->
                            // Result<str,str>`), keep the wrapper but blank the
                            // inner to `Unknown`. That preserves enough shape for
                            // the concrete-wrapper-into-deferred-slot guard to
                            // fire (passing `dict_get(d,k)` straight into a `Dict`
                            // slot is a type error) while still treating the
                            // payload as unresolved everywhere else.
                            return match &sig.ret {
                                Type::Option(_) => Type::Option(Box::new(Type::Unknown)),
                                Type::Result(_, _) => {
                                    Type::Result(Box::new(Type::Unknown), Box::new(Type::Unknown))
                                }
                                _ => Type::Unknown,
                            };
                        }
                        return sig.ret.clone();
                    }
                }
                Type::Unknown
            }
            Expr::Array(elems) => {
                // Resolve the element type from the first element so `a[i].foo`
                // (field access on a scalar element) can be caught — a bare
                // `Slice(Unknown)` would lose the element type and let it slip to
                // a runtime panic. Empty literal → Unknown element.
                let elem_ty = elems
                    .first()
                    .map(|e| self.resolve_expr_type(e, &format!("{node_path}.elem_0"), scope))
                    .unwrap_or(Type::Unknown);
                Type::Slice(Box::new(elem_ty))
            }
            Expr::Tuple(elems) => {
                // Recurse per element so `t.0`, `t.1` can be checked structurally
                // even when inference didn't register an expr_types entry.
                let tys: Vec<Type> = elems
                    .iter()
                    .enumerate()
                    .map(|(i, e)| {
                        self.resolve_expr_type(e, &format!("{node_path}.elem_{i}"), scope)
                    })
                    .collect();
                Type::Tuple(tys)
            }
            // Chained/nested field access: resolve the receiver's struct, then
            // look up the field's declared type. Without this arm `p.x` falls to
            // `Type::Unknown` below, so `p.x.y` (accessing `.y` on the i64 field
            // `p.x`) resolved its receiver to Unknown → check_field_access's
            // Unknown arm deferred to inference, which ALSO misses it → the error
            // slipped to a runtime "field access on non-struct" panic. Resolving
            // the field type here lets the existing R11 check flag it statically.
            Expr::FieldAccess { receiver, field } => {
                let recv_ty =
                    self.resolve_expr_type(receiver, &format!("{node_path}.receiver"), scope);
                match recv_ty {
                    Type::Struct(sname) => self
                        .struct_fields
                        .get(&sname)
                        .and_then(|fields| {
                            fields
                                .iter()
                                .find(|(n, _)| n == field)
                                .map(|(_, t)| t.clone())
                        })
                        // Unknown field name: leave Unknown so the R11 field-
                        // existence check (which owns that error + the known-
                        // fields hint) reports it, not this resolver.
                        .unwrap_or(Type::Unknown),
                    // Tuple element type when the field is a valid numeric index.
                    Type::Tuple(elems) => field
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| elems.get(i).cloned())
                        .unwrap_or(Type::Unknown),
                    _ => Type::Unknown,
                }
            }
            // Indexing a slice/array yields the element type, so `a[0].foo`
            // resolves `a[0]` to the element and the R11 check can flag a field
            // access on a non-struct element (else it slips to a runtime "field
            // access on non-struct" panic, same class as the FieldAccess arm).
            Expr::Index { receiver, index } => {
                let _ = index;
                let recv_ty =
                    self.resolve_expr_type(receiver, &format!("{node_path}.receiver"), scope);
                match recv_ty {
                    Type::Slice(elem) => *elem,
                    // str/other receivers are not indexable (the R11 Index check
                    // reports E0402); leave Unknown so no wrong type flows on.
                    _ => Type::Unknown,
                }
            }
            // An `if`/`else` expression's type is its branch type, so
            // `(if c { 5 } else { 6 }).foo` can be checked. Only commit to a type
            // when BOTH branches resolve to the SAME concrete type (what a real
            // unifier requires); a mismatch or any Unknown stays Unknown and lets
            // inference own it. (This is now safe to feed downstream: the E0307
            // body-tail check is gated on the exact fn-body path, so a match-arm
            // if-expr no longer leaks into the return-type comparison.)
            Expr::If {
                then,
                else_: Some(e),
                ..
            } => {
                let then_ty = self.resolve_expr_type(then, &format!("{node_path}.then"), scope);
                let else_ty = self.resolve_expr_type(e, &format!("{node_path}.else"), scope);
                if then_ty == else_ty && !matches!(then_ty, Type::Unknown) {
                    then_ty
                } else {
                    Type::Unknown
                }
            }
            // No else branch → the if is `()`-typed; leave Unknown for inference.
            Expr::If { else_: None, .. } => Type::Unknown,
            // `expr?` unwraps a Result<T,E> / Option<T> to its inner T, so
            // `get()?.foo` (field access on the unwrapped scalar) can be checked
            // instead of slipping to a runtime panic.
            Expr::Question(inner) => {
                let inner_ty = self.resolve_expr_type(inner, &format!("{node_path}.inner"), scope);
                match inner_ty {
                    Type::Result(ok, _) => *ok,
                    Type::Option(t) => *t,
                    _ => Type::Unknown,
                }
            }
            // A `match` expression's type is its arms' common type — same
            // unify-or-Unknown rule as `if` (so `(match s { … }).foo` and a
            // let-bound match result can be field-checked). Empty/none-agreeing
            // → Unknown.
            Expr::Match { arms, .. } => {
                let mut arm_ty: Option<Type> = None;
                for (i, arm) in arms.iter().enumerate() {
                    let t = self.resolve_expr_type(
                        &arm.body,
                        &format!("{node_path}.arm_{i}.body"),
                        scope,
                    );
                    if matches!(t, Type::Unknown) {
                        arm_ty = None;
                        break;
                    }
                    match &arm_ty {
                        None => arm_ty = Some(t),
                        Some(prev) if *prev == t => {}
                        Some(_) => {
                            arm_ty = None;
                            break;
                        }
                    }
                }
                arm_ty.unwrap_or(Type::Unknown)
            }
            // NOTE: deliberately NO `Expr::BinOp` arm. Resolving an arithmetic
            // binop to its operand type to catch `(1 + 2).foo` is unsound here:
            // an `Uncertain<T>`/`Temporal<T>` operand makes `a + b` stay
            // Uncertain/Temporal (and carry `.value`/`.confidence`), but a naive
            // operand-type resolution collapses it to the inner `i64`, which then
            // false-flags `(ua + ub).value` as a non-field. Comparison/logical
            // ops are always bool but `(a < b).foo` is too rare to special-case.
            // Leave binops to inference.
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
/// Rewrite `Type::Struct(n)` → `Type::Enum(n)` (recursively, through the common
/// type containers) when `n` is a known enum. `axon_type_to_type` is context-free
/// and defaults unknown named types to `Struct`; enum and struct names don't
/// overlap, so this is a safe normalization for declared annotations.
fn enumify(t: Type, enums: &[String]) -> Type {
    match t {
        Type::Struct(n) if enums.iter().any(|e| e == &n) => Type::Enum(n),
        Type::Option(i) => Type::Option(Box::new(enumify(*i, enums))),
        Type::Slice(i) => Type::Slice(Box::new(enumify(*i, enums))),
        Type::Chan(i) => Type::Chan(Box::new(enumify(*i, enums))),
        Type::Result(o, e) => {
            Type::Result(Box::new(enumify(*o, enums)), Box::new(enumify(*e, enums)))
        }
        other => other,
    }
}

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
            "Decimal" => Type::Decimal,
            "bool" => Type::Bool,
            "str" | "String" => Type::Str,
            "()" | "unit" => Type::Unit,
            "never" | "Never" | "!" => Type::Never,
            other => {
                if DEFERRED_PREFIXES.iter().any(|p| other.starts_with(p)) {
                    Type::Deferred(other.to_string())
                } else {
                    Type::Struct(other.to_string())
                }
            }
        },
        AxonType::Result { ok, err } => Type::Result(
            Box::new(axon_type_to_type(ok)),
            Box::new(axon_type_to_type(err)),
        ),
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
        // R17 HAL
        AxonType::RawPtr(inner) => Type::RawPtr(Box::new(axon_type_to_type(inner))),
    }
}

// ── AxonType helpers for E0501/E0502/E0503 ────────────────────────────────────

/// Returns a human-readable name for an `AxonType`, used in error messages.
/// The name under which methods are registered/looked up for a value of this
/// type — matching the interpreter's `Value::type_name()` / `type_name_of()`
/// keys, so `type_methods[key]` reflects exactly the methods callable at
/// runtime. Returns `None` for types where the checker can't be sure of the
/// receiver (Unknown/Var/Deferred) or which dispatch through other paths
/// (channels = builtin methods; fn/dyn = not method receivers) — those are left
/// to inference / the runtime to avoid false E0403s.
/// A canonical key for a match pattern's head constructor, returned ONLY when
/// the pattern conclusively determines its coverage — so a second arm with the
/// same key is provably unreachable (W0004). Returns `None` for patterns where a
/// repeat could still be reachable (wildcard/ident bind-alls — though those make
/// LATER arms unreachable, not themselves; sub-patterns that differ, e.g.
/// `Some(1)` vs `Some(2)`; struct/variant patterns whose fields bind sub-patterns
/// that could differ). Conservative by design: a missed duplicate is silent, a
/// false "unreachable" would be wrong.
fn conclusive_pattern_key(p: &Pattern) -> Option<String> {
    match p {
        // A fieldless or all-binding variant/struct pattern: the head alone
        // decides coverage. `S::A`, `S::A { x }` (x just binds) → key on the name.
        // If any field carries a NESTED non-binding sub-pattern, bail (it could
        // differ between arms), so only accept fields that are wildcard/ident.
        Pattern::Struct { name, fields } => {
            if fields
                .iter()
                .all(|(_, fp)| matches!(fp, Pattern::Wildcard | Pattern::Ident(_)))
            {
                Some(name.to_string())
            } else {
                None
            }
        }
        Pattern::None => Some("None".to_string()),
        // A literal pattern is conclusive (`0`, `"x"`, `true`).
        Pattern::Literal(lit) => Some(format!("lit:{lit:?}")),
        // Some(x)/Ok(x)/Err(x) ONLY when the inner is a bind-all (so the head
        // alone decides); a deeper sub-pattern (`Some(1)`) could differ.
        Pattern::Some(inner) if matches!(**inner, Pattern::Wildcard | Pattern::Ident(_)) => {
            Some("Some".to_string())
        }
        Pattern::Ok(inner) if matches!(**inner, Pattern::Wildcard | Pattern::Ident(_)) => {
            Some("Ok".to_string())
        }
        Pattern::Err(inner) if matches!(**inner, Pattern::Wildcard | Pattern::Ident(_)) => {
            Some("Err".to_string())
        }
        // Wildcard/Ident bind-alls, deep Some/Ok/Err, tuples: not keyed (a repeat
        // of a bind-all is handled by exhaustiveness, not duplicate detection).
        _ => None,
    }
}

/// The scalar "kind" a subject type belongs to for literal-pattern matching, or
/// `None` for non-scalar / inference-pending types (where no E0405 should fire).
/// `i64`/`i32` collapse to one "int" kind (mutually compatible by widening).
fn scalar_kind(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::I64 | Type::I32 => Some("int"),
        Type::F64 => Some("float"),
        Type::Bool => Some("bool"),
        Type::Str => Some("str"),
        _ => None,
    }
}

/// The scalar kind of a literal pattern — aligned with `scalar_kind` so an
/// int-literal pattern against an int subject compares equal.
fn literal_scalar_kind(lit: &crate::ast::Literal) -> &'static str {
    use crate::ast::Literal;
    match lit {
        Literal::Int(_) => "int",
        Literal::Float(_) => "float",
        Literal::Bool(_) => "bool",
        Literal::Str(_) => "str",
        Literal::Decimal(_) => "decimal",
    }
}

/// True when an expression unconditionally diverts control flow, so any
/// statement after it in the same block is unreachable. Conservative: only the
/// bare control-flow forms (`return`, `break`, `continue`). An `if` whose every
/// branch diverts is NOT treated as a terminator here (that needs branch
/// analysis); this catches the common straight-line dead-code case.
fn is_terminator(e: &Expr) -> bool {
    matches!(e, Expr::Return(_) | Expr::Break | Expr::Continue)
}

/// Best-effort constant folder for INTEGER expressions, used to catch a
/// divide-by-a-constant-zero (E0407). Returns `Some(n)` only when the expression
/// is a pure integer constant (literal, unary neg, or +/-/*/%/// over constants);
/// `None` for anything non-constant (a variable, a call, a float). A nested
/// division by zero folds to `None` (we don't panic the checker over it; the
/// outer check still fires on whatever it can fold).
fn const_eval_int(e: &Expr) -> Option<i64> {
    use crate::ast::{BinOp, Literal, UnaryOp};
    match e {
        Expr::Literal(Literal::Int(n)) => Some(*n),
        Expr::UnaryOp {
            op: UnaryOp::Neg,
            operand,
        } => const_eval_int(operand)?.checked_neg(),
        Expr::BinOp { op, left, right } => {
            let l = const_eval_int(left)?;
            let r = const_eval_int(right)?;
            match op {
                BinOp::Add => l.checked_add(r),
                BinOp::Sub => l.checked_sub(r),
                BinOp::Mul => l.checked_mul(r),
                // A zero divisor here would itself be a div-by-zero; don't fold
                // (avoid a checker panic) — return None so we simply can't prove
                // the OUTER divisor's value through it.
                BinOp::Div if r != 0 => l.checked_div(r),
                BinOp::Rem if r != 0 => l.checked_rem(r),
                _ => None,
            }
        }
        // Pure, total integer bound builtins, so a CONSTANT refinement obligation
        // built from them (e.g. `let p: Pos = max_i64(a, 0)`) is discharged at
        // compile time (E1209/E1201) instead of deferring to the runtime gate.
        // Semantics match the interpreter EXACTLY: i64::min/max, and a CHECKED abs
        // (so `abs_i64(i64::MIN)` overflows → None → stays deferred to the runtime
        // panic, never a wrong fold). Keeps the SMT / comptime / checker constant
        // folders consistent on these builtins.
        Expr::Call { callee, args, .. } => {
            let Expr::Ident(name) = callee.as_ref() else {
                return None;
            };
            match (name.as_str(), args.as_slice()) {
                ("min_i64", [a, b]) => Some(const_eval_int(a)?.min(const_eval_int(b)?)),
                ("max_i64", [a, b]) => Some(const_eval_int(a)?.max(const_eval_int(b)?)),
                ("abs_i64", [x]) => {
                    let v = const_eval_int(x)?;
                    if v < 0 {
                        v.checked_neg()
                    } else {
                        Some(v)
                    }
                }
                _ => None,
            }
        }
        _ => None,
    }
}

fn method_lookup_key(ty: &Type) -> Option<&'static str> {
    match ty {
        Type::Struct(_) | Type::Enum(_) => None, // handled inline (needs the name)
        Type::I64 => Some("i64"),
        Type::I32 => Some("i32"),
        Type::F64 => Some("f64"),
        Type::Bool => Some("bool"),
        Type::Str => Some("str"),
        Type::Slice(_) => Some("[]"),
        Type::Tuple(_) => Some("tuple"),
        Type::Option(_) => Some("Option"),
        Type::Result(_, _) => Some("Result"),
        // Unknown / Var / Deferred / Fn / Chan / DynTrait / Uncertain / Temporal
        // / Unit / Dict: don't risk a false positive.
        _ => None,
    }
}

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
        AxonType::RawPtr(inner) => format!("*{}", axon_type_name(inner)),
    }
}

/// Returns a display string for an `AxonType` suitable for error messages.
/// Identical to `axon_type_name` for now; separated so they can diverge.
/// Is `name` an **exfiltration sink** — a builtin that sends data off the
/// program boundary, where a `@[sensitive]` value must never go (E1206)?
/// Covers: AI calls (to a model), `write_file` (to disk — persisting PII), and
/// `exec` (to a spawned process — e.g. piping data to `curl`). Returns the
/// human-readable boundary name for the diagnostic.
/// Collect every expression in RETURN position of `body`: the STRUCTURAL TAIL of
/// a block (recursing through nested blocks / if-else branches / match arms),
/// plus every explicit `return <e>` reachable anywhere. Used by the R6
/// return-value taint analysis.
fn collect_return_exprs<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
    // Every explicit `return <e>` anywhere in the body is a return position.
    collect_explicit_returns(e, out);
    // Plus the structural tail.
    collect_tail_expr(e, out);
}

/// The structural tail expression(s) of `e` (the value a block "falls off" to),
/// recursing into block-last / if-branches / match-arm-bodies.
fn collect_tail_expr<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
    use crate::ast::Expr as E;
    match e {
        E::Block(stmts) => {
            if let Some(last) = stmts.last() {
                collect_tail_expr(&last.expr, out);
            }
        }
        E::If { then, else_, .. } => {
            collect_tail_expr(then, out);
            if let Some(els) = else_ {
                collect_tail_expr(els, out);
            }
        }
        E::Match { arms, .. } => {
            for arm in arms {
                collect_tail_expr(&arm.body, out);
            }
        }
        // A `return`/leaf/value expression is itself the tail (an explicit
        // return is also picked up by collect_explicit_returns; harmless dup).
        other => out.push(other),
    }
}

/// Collect every explicit `return <e>` expression nested anywhere under `e`.
/// (A direct match rather than via `each_subexpr`, whose callback lifetime can't
/// thread the collected references back out.)
fn collect_explicit_returns<'e>(e: &'e Expr, out: &mut Vec<&'e Expr>) {
    use crate::ast::Expr as E;
    match e {
        E::Return(Some(inner)) => {
            out.push(inner);
            collect_explicit_returns(inner, out);
        }
        E::Block(stmts) | E::While { body: stmts, .. } | E::WhileLet { body: stmts, .. } => {
            for s in stmts {
                collect_explicit_returns(&s.expr, out);
            }
        }
        E::WithHandler { handler, body } => {
            if let crate::ast::HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                for arm in arms.iter().chain(return_arm.as_deref()) {
                    collect_explicit_returns(&arm.body, out);
                }
            }
            collect_explicit_returns(body, out);
        }
        E::For {
            start, end, body, ..
        } => {
            collect_explicit_returns(start, out);
            collect_explicit_returns(end, out);
            for s in body {
                collect_explicit_returns(&s.expr, out);
            }
        }
        E::Let { value, .. } | E::Own { value, .. } | E::RefBind { value, .. } => {
            collect_explicit_returns(value, out)
        }
        E::Call { callee, args, .. } => {
            collect_explicit_returns(callee, out);
            for a in args {
                collect_explicit_returns(a, out);
            }
        }
        E::MethodCall { receiver, args, .. } => {
            collect_explicit_returns(receiver, out);
            for a in args {
                collect_explicit_returns(a, out);
            }
        }
        E::BinOp { left, right, .. } => {
            collect_explicit_returns(left, out);
            collect_explicit_returns(right, out);
        }
        E::UnaryOp { operand, .. } => collect_explicit_returns(operand, out),
        E::Question(inner) | E::Spawn(inner) | E::Comptime(inner) => {
            collect_explicit_returns(inner, out)
        }
        E::Match { subject, arms } => {
            collect_explicit_returns(subject, out);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_explicit_returns(g, out);
                }
                collect_explicit_returns(&arm.body, out);
            }
        }
        E::If { cond, then, else_ } => {
            collect_explicit_returns(cond, out);
            collect_explicit_returns(then, out);
            if let Some(els) = else_ {
                collect_explicit_returns(els, out);
            }
        }
        E::Select(arms) => {
            for arm in arms {
                collect_explicit_returns(&arm.recv, out);
                collect_explicit_returns(&arm.body, out);
            }
        }
        E::Lambda { body, .. } => collect_explicit_returns(body, out),
        E::FieldAccess { receiver, .. } => collect_explicit_returns(receiver, out),
        E::Index { receiver, index } => {
            collect_explicit_returns(receiver, out);
            collect_explicit_returns(index, out);
        }
        E::Tuple(elems) | E::Array(elems) => {
            for el in elems {
                collect_explicit_returns(el, out);
            }
        }
        E::FmtStr { parts } => {
            for p in parts {
                if let crate::ast::FmtPart::Expr(inner) = p {
                    collect_explicit_returns(inner, out);
                }
            }
        }
        E::Ok(inner) | E::Err(inner) | E::Some(inner) => collect_explicit_returns(inner, out),
        E::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_explicit_returns(v, out);
            }
        }
        E::Assign { value, .. } => collect_explicit_returns(value, out),
        E::AssignTo { place, value } => {
            collect_explicit_returns(place, out);
            collect_explicit_returns(value, out);
        }
        E::Return(None)
        | E::Ident(_)
        | E::Literal(_)
        | E::None
        | E::Break
        | E::Continue
        | E::InlineAsm { .. } => {}
    }
}

/// True when expression `e` carries the identifier `name` in a "this value is
/// the argument" sense: the bare identifier, a field access whose receiver
/// carries it (`u.email`), an index/cast of it, or a string interpolation that
/// mentions it. Conservative for taint: a match/call that merely contains the
/// name elsewhere is not counted (only flow-through forms).
fn arg_carries_ident(e: &Expr, name: &str) -> bool {
    match e {
        Expr::Ident(n) => n == name,
        Expr::FieldAccess { receiver, .. } => arg_carries_ident(receiver, name),
        Expr::Index { receiver, .. } => arg_carries_ident(receiver, name),
        Expr::FmtStr { parts } => parts.iter().any(|p| match p {
            crate::ast::FmtPart::Expr(inner) => arg_carries_ident(inner, name),
            _ => false,
        }),
        _ => false,
    }
}

/// Invoke `f` on each DIRECT sub-expression of `e` (one level). Used by the
/// taint walker to recurse uniformly without re-listing the AST per call site.
fn each_subexpr(e: &Expr, f: &mut dyn FnMut(&Expr)) {
    use crate::ast::Expr as E;
    match e {
        E::Block(stmts) | E::While { body: stmts, .. } | E::WhileLet { body: stmts, .. } => {
            for s in stmts {
                f(&s.expr);
            }
        }
        E::WithHandler { handler, body } => {
            if let crate::ast::HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                for arm in arms.iter().chain(return_arm.as_deref()) {
                    f(&arm.body);
                }
            }
            f(body);
        }
        E::Let { value, .. } | E::Own { value, .. } | E::RefBind { value, .. } => f(value),
        E::Call { callee, args, .. } => {
            f(callee);
            for a in args {
                f(a);
            }
        }
        E::MethodCall { receiver, args, .. } => {
            f(receiver);
            for a in args {
                f(a);
            }
        }
        E::BinOp { left, right, .. } => {
            f(left);
            f(right);
        }
        E::UnaryOp { operand, .. } => f(operand),
        E::Question(inner) | E::Spawn(inner) | E::Comptime(inner) => f(inner),
        E::Match { subject, arms } => {
            f(subject);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    f(g);
                }
                f(&arm.body);
            }
        }
        E::If { cond, then, else_ } => {
            f(cond);
            f(then);
            if let Some(e) = else_ {
                f(e);
            }
        }
        E::Select(arms) => {
            for arm in arms {
                f(&arm.recv);
                f(&arm.body);
            }
        }
        E::Lambda { body, .. } => f(body),
        E::Return(Some(inner)) => f(inner),
        E::FieldAccess { receiver, .. } => f(receiver),
        E::Index { receiver, index } => {
            f(receiver);
            f(index);
        }
        E::Tuple(elems) | E::Array(elems) => {
            for el in elems {
                f(el);
            }
        }
        E::FmtStr { parts } => {
            for p in parts {
                if let crate::ast::FmtPart::Expr(inner) = p {
                    f(inner);
                }
            }
        }
        E::Ok(inner) | E::Err(inner) | E::Some(inner) => f(inner),
        E::StructLit { fields, .. } => {
            for (_, v) in fields {
                f(v);
            }
        }
        E::Assign { value, .. } => f(value),
        E::AssignTo { place, value } => {
            f(place);
            f(value);
        }
        E::For {
            start, end, body, ..
        } => {
            f(start);
            f(end);
            for s in body {
                f(&s.expr);
            }
        }
        E::Ident(_)
        | E::Literal(_)
        | E::None
        | E::Return(None)
        | E::Break
        | E::Continue
        | E::InlineAsm { .. } => {}
    }
}

fn exfiltration_sink_kind(name: &str) -> Option<&'static str> {
    if name == "ai_complete" || name.starts_with("ai_extract") {
        Some("AI call")
    } else if name == "write_file" {
        Some("file write")
    } else if name == "exec" {
        Some("process exec")
    } else {
        None
    }
}

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
                && aa
                    .iter()
                    .zip(ab.iter())
                    .all(|(x, y)| axon_types_compatible(x, y))
        }
        (AxonType::Result { ok: oa, err: ea }, AxonType::Result { ok: ob, err: eb }) => {
            axon_types_compatible(oa, ob) && axon_types_compatible(ea, eb)
        }
        (AxonType::Option(ia), AxonType::Option(ib)) => axon_types_compatible(ia, ib),
        (AxonType::Chan(ia), AxonType::Chan(ib)) => axon_types_compatible(ia, ib),
        (AxonType::Slice(ia), AxonType::Slice(ib)) => axon_types_compatible(ia, ib),
        (AxonType::Ref(ia), AxonType::Ref(ib)) => axon_types_compatible(ia, ib),
        (AxonType::RawPtr(ia), AxonType::RawPtr(ib)) => axon_types_compatible(ia, ib),
        (AxonType::DynTrait(na), AxonType::DynTrait(nb)) => na == nb,
        (
            AxonType::Fn {
                params: pa,
                ret: ra,
            },
            AxonType::Fn {
                params: pb,
                ret: rb,
            },
        ) => {
            pa.len() == pb.len()
                && pa
                    .iter()
                    .zip(pb.iter())
                    .all(|(x, y)| axon_types_compatible(x, y))
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
            if d <= 3 {
                Option::Some((d, c.as_str()))
            } else {
                Option::None
            }
        })
        .min_by_key(|(d, _)| *d)
        .map(|(_, s)| s)
}

// ── Phase 13 Slice 2: distribution moment + CDF helpers (checker-side) ────────

fn refine_val_to_f64(v: &RefineVal) -> Option<f64> {
    match v {
        RefineVal::Float(f) => Some(*f),
        RefineVal::Int(n) => Some(*n as f64),
        _ => None,
    }
}

/// Compute E[dist] or Var[dist] from a field-map of f64 values.
fn checker_dist_moment(tag: &str, fields: &HashMap<String, f64>) -> Option<f64> {
    // Gaussian: mu, sigma → E=mu, Var=sigma²
    if let (Some(&mu), Some(&sigma)) = (fields.get("mu"), fields.get("sigma")) {
        return match tag {
            "E" => Some(mu),
            "Var" => Some(sigma * sigma),
            _ => None,
        };
    }
    // Beta: alpha, beta_b → E=alpha/(alpha+beta_b), Var=...
    if let (Some(&alpha), Some(&beta_b)) = (fields.get("alpha"), fields.get("beta_b")) {
        let s = alpha + beta_b;
        return match tag {
            "E" => Some(alpha / s),
            "Var" => Some((alpha * beta_b) / (s * s * (s + 1.0))),
            _ => None,
        };
    }
    None
}

/// Compute CDF P(X <= k) from a field-map for known distributions.
fn checker_dist_cdf(fields: &HashMap<String, f64>, k: f64) -> Option<f64> {
    // Gaussian
    if let (Some(&mu), Some(&sigma)) = (fields.get("mu"), fields.get("sigma")) {
        if sigma > 0.0 {
            let z = (k - mu) / (sigma * std::f64::consts::SQRT_2);
            return Some(0.5 * (1.0 + checker_erf(z)));
        }
    }
    // Beta
    if let (Some(&alpha), Some(&beta_b)) = (fields.get("alpha"), fields.get("beta_b")) {
        if alpha > 0.0 && beta_b > 0.0 {
            // Reuse the same beta CDF implementation via a simpler approximation
            // for the checker-side (constant fold only): use the closed-form from
            // the spec §4.2 for constant parameters.
            // For k outside [0,1] the Beta CDF is 0 or 1:
            if k <= 0.0 {
                return Some(0.0);
            }
            if k >= 1.0 {
                return Some(1.0);
            }
            // Use regularized incomplete beta (Lentz CF), same algo as interp side
            return Some(checker_beta_cdf(alpha, beta_b, k));
        }
    }
    None
}

fn checker_erf(x: f64) -> f64 {
    let t = 1.0 / (1.0 + 0.3275911 * x.abs());
    let poly = t
        * (0.254829592
            + t * (-0.284496736 + t * (1.421413741 + t * (-1.453152027 + t * 1.061405429))));
    let sign = if x < 0.0 { -1.0 } else { 1.0 };
    sign * (1.0 - poly * (-x * x).exp())
}

fn checker_beta_cdf(alpha: f64, beta_b: f64, x: f64) -> f64 {
    if x > (alpha + 1.0) / (alpha + beta_b + 2.0) {
        return 1.0 - checker_beta_cdf(beta_b, alpha, 1.0 - x);
    }
    let ln_beta =
        checker_ln_gamma(alpha) + checker_ln_gamma(beta_b) - checker_ln_gamma(alpha + beta_b);
    let front = (alpha * x.ln() + beta_b * (1.0 - x).ln() - ln_beta).exp() / alpha;
    front * checker_beta_cf(alpha, beta_b, x)
}

fn checker_beta_cf(alpha: f64, beta_b: f64, x: f64) -> f64 {
    let max_iter = 200;
    let eps = 1e-10;
    let mut c = 1.0_f64;
    let mut d = 1.0 - (alpha + beta_b) * x / (alpha + 1.0);
    d = 1.0
        / if d.abs() < f64::MIN_POSITIVE {
            f64::MIN_POSITIVE
        } else {
            d
        };
    let mut f = d;
    for m in 1..=max_iter {
        let m = m as f64;
        let num = m * (beta_b - m) * x / ((alpha + 2.0 * m - 1.0) * (alpha + 2.0 * m));
        d = 1.0 + num * d;
        c = 1.0 + num / c;
        d = 1.0
            / if d.abs() < f64::MIN_POSITIVE {
                f64::MIN_POSITIVE
            } else {
                d
            };
        c = if c.abs() < f64::MIN_POSITIVE {
            f64::MIN_POSITIVE
        } else {
            c
        };
        f *= d * c;
        let num =
            -(alpha + m) * (alpha + beta_b + m) * x / ((alpha + 2.0 * m) * (alpha + 2.0 * m + 1.0));
        d = 1.0 + num * d;
        c = 1.0 + num / c;
        d = 1.0
            / if d.abs() < f64::MIN_POSITIVE {
                f64::MIN_POSITIVE
            } else {
                d
            };
        c = if c.abs() < f64::MIN_POSITIVE {
            f64::MIN_POSITIVE
        } else {
            c
        };
        let delta = d * c;
        f *= delta;
        if (delta - 1.0).abs() < eps {
            break;
        }
    }
    f
}

fn checker_ln_gamma(x: f64) -> f64 {
    let p = [
        676.5203681218851_f64,
        -1259.1392167224028,
        771.323_428_777_653_1,
        -176.615_029_162_140_6,
        12.507343278686905,
        -0.13857109526572012,
        9.984_369_578_019_572e-6,
        1.5056327351493116e-7,
    ];
    let x = x - 1.0;
    let mut a = 0.999_999_999_999_809_9_f64;
    for (i, &p_i) in p.iter().enumerate() {
        a += p_i / (x + i as f64 + 1.0);
    }
    let t = x + 7.5;
    0.5 * (2.0 * std::f64::consts::PI).ln() + (x + 0.5) * t.ln() - t + a.ln()
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
            verify: None,
            effect_row: None,
            span: crate::span::Span::dummy(),
        })
    }

    fn param(name: &str, ty: AxonType) -> crate::ast::Param {
        crate::ast::Param {
            name: name.to_string(),
            ty,
            span: crate::span::Span::dummy(),
        }
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
            FnSig {
                params: vec![Type::I32],
                ret: Type::I32,
            },
        );

        let mut ctx = mk_ctx(sigs);

        let program = make_program(vec![simple_fn(
            "caller",
            vec![param(
                "opt_val",
                AxonType::Option(Box::new(AxonType::Named("i32".into()))),
            )],
            Option::Some(AxonType::Named("i32".into())),
            block(vec![Expr::Call {
                callee: Box::new(ident("add_one")),
                args: vec![ident("opt_val")],
                tier: None,
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
                Expr::Call {
                    callee: Box::new(ident("may_fail")),
                    args: vec![],
                    tier: None,
                },
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
            vec![param(
                "x",
                AxonType::Option(Box::new(AxonType::Named("i32".into()))),
            )],
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
            errors
                .iter()
                .any(|e| e.code == E0304 && e.message.contains("None")),
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
                tier: None,
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
            FnSig {
                params: vec![Type::Bool],
                ret: Type::Unit,
            },
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
                    tier: None,
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
                Expr::Let {
                    ty: None,
                    name: "x".into(),
                    value: Box::new(lit_int(42)),
                },
                Expr::FieldAccess {
                    receiver: Box::new(ident("x")),
                    field: "foo".into(),
                },
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
            FnSig {
                params: vec![Type::I32],
                ret: Type::Unit,
            },
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
                    tier: None,
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
            errors
                .iter()
                .any(|e| e.code == E0304 && e.message.contains("Err")),
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
                ty: None,
                name: "r".into(),
                value: Box::new(Expr::Call {
                    callee: Box::new(ident("may_fail")),
                    args: vec![],
                    tier: None,
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
            vec![param(
                "x",
                AxonType::Option(Box::new(AxonType::Named("i32".into()))),
            )],
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
            FnSig {
                params: vec![Type::I64],
                ret: Type::Unit,
            },
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
                    tier: None,
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
            FnSig {
                params: vec![Type::I64],
                ret: Type::Unit,
            },
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
                    tier: None,
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
            verify: None,
            effect_row: None,
            span: crate::span::Span::dummy(),
        });
        let program = make_program(vec![fndef]);
        let mut ctx = mk_ctx(HashMap::new());
        let errors = run_with_types(&mut ctx, &program, HashMap::new());
        let r08: Vec<_> = errors.iter().filter(|e| e.code == E0308).collect();
        assert!(
            r08.is_empty(),
            "generic param T should not produce E0308: {r08:?}"
        );
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

    fn make_impl_block(trait_name: &str, for_type: AxonType, methods: Vec<FnDef>) -> Item {
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
            verify: None,
            effect_row: None,
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
            FnSig {
                params: vec![Type::TypeParam("T".into())],
                ret: Type::Unit,
            },
        );
        let mut ctx = mk_ctx(sigs);

        // Register the bound on "show".
        ctx.fn_bounds
            .insert("show".into(), vec![("T".into(), vec!["Display".into()])]);
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
                    tier: None,
                },
                lit_int(0),
            ]),
        )]);

        // Inject arg type into expr_types so resolve_expr_type returns Struct("Qux").
        let mut expr_types = HashMap::new();
        expr_types.insert(
            "#fn_caller.body.stmt_0.arg_0".to_string(),
            Type::Struct("Qux".into()),
        );
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
            FnSig {
                params: vec![Type::TypeParam("T".into())],
                ret: Type::Unit,
            },
        );
        let mut ctx = mk_ctx(sigs);
        ctx.fn_bounds
            .insert("show".into(), vec![("T".into(), vec!["Display".into()])]);
        // Register Qux as implementing Display.
        ctx.impl_table
            .entry("Qux".into())
            .or_default()
            .insert("Display".into());

        let program = make_program(vec![simple_fn(
            "caller",
            vec![],
            None,
            block(vec![
                Expr::Call {
                    callee: Box::new(ident("show")),
                    args: vec![ident("qux_val")],
                    tier: None,
                },
                lit_int(0),
            ]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert(
            "#fn_caller.body.stmt_0.arg_0".to_string(),
            Type::Struct("Qux".into()),
        );
        expr_types.insert("#fn_caller.body.stmt_0".to_string(), Type::Unit);
        expr_types.insert("#fn_caller.body.stmt_1".to_string(), Type::I64);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        let e504: Vec<_> = errors.iter().filter(|e| e.code == E0504).collect();
        assert!(
            e504.is_empty(),
            "Qux implements Display, should not produce E0504: {e504:?}"
        );
    }

    #[test]
    fn e0501_unknown_trait() {
        // impl of a trait that doesn't exist in the program.
        let program = make_program(vec![make_impl_block(
            "NonExistent",
            AxonType::Named("Foo".into()),
            vec![make_fndef("hello", vec![], None)],
        )]);
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
        let trait_def = make_trait_def(
            "Greet",
            vec![
                make_trait_method(
                    "hello",
                    vec![param("name", AxonType::Named("str".into()))],
                    Some(AxonType::Named("str".into())),
                ),
                make_trait_method("farewell", vec![], Some(AxonType::Named("str".into()))),
            ],
        );
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
            errors
                .iter()
                .any(|e| e.code == E0502 && e.message.contains("farewell")),
            "expected E0502 for missing 'farewell', got: {errors:?}"
        );
    }

    #[test]
    fn e0503_param_count_mismatch() {
        // impl method has 0 params; trait declares 1 param.
        let trait_def = make_trait_def(
            "Greet",
            vec![make_trait_method(
                "hello",
                vec![param("name", AxonType::Named("str".into()))],
                Some(AxonType::Named("str".into())),
            )],
        );
        let impl_block = make_impl_block(
            "Greet",
            AxonType::Named("Baz".into()),
            vec![make_fndef(
                "hello",
                vec![],
                Some(AxonType::Named("str".into())),
            )],
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
        let trait_def = make_trait_def(
            "Greet",
            vec![make_trait_method(
                "hello",
                vec![param("name", AxonType::Named("str".into()))],
                Some(AxonType::Named("str".into())),
            )],
        );
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
        let trait_errors: Vec<_> = errors
            .iter()
            .filter(|e| e.code == E0501 || e.code == E0502 || e.code == E0503)
            .collect();
        assert!(
            trait_errors.is_empty(),
            "valid impl should not produce trait errors, got: {trait_errors:?}"
        );
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
            FnSig {
                params: vec![Type::I32, Type::Bool],
                ret: Type::I32,
            },
        );
        let mut ctx = mk_ctx(sigs);

        let program = make_program(vec![simple_fn(
            "caller",
            vec![],
            Option::Some(AxonType::Named("i32".into())),
            block(vec![Expr::Call {
                callee: Box::new(ident("two_arg")),
                args: vec![lit_int(1)],
                tier: None,
            }]),
        )]);

        let errors = run(&mut ctx, &program);
        let e0305 = errors
            .iter()
            .find(|e| e.code == E0305)
            .expect("expected E0305");
        // New message: "function `two_arg` takes 2 arguments but 1 was supplied"
        assert!(
            e0305.message.contains("`two_arg`"),
            "E0305 message should name the function: {}",
            e0305.message
        );
        assert!(
            e0305.message.contains("2 arguments"),
            "E0305 message should state expected arity: {}",
            e0305.message
        );
        assert!(
            e0305.message.contains("1 was supplied"),
            "E0305 message should state observed arity: {}",
            e0305.message
        );
        // Fix should spell out the signature.
        let fix = e0305.fix.as_deref().unwrap_or("");
        assert!(
            fix.contains("two_arg") && fix.contains("i32") && fix.contains("bool"),
            "E0305 fix should render the expected signature: {fix}"
        );
    }

    #[test]
    fn e0306_message_names_arg_index_and_types() {
        let mut sigs = HashMap::new();
        sigs.insert(
            "wants_bool".to_string(),
            FnSig {
                params: vec![Type::Bool],
                ret: Type::Unit,
            },
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
                    tier: None,
                },
                Expr::Literal(Literal::Bool(true)),
            ]),
        )]);

        let mut expr_types = HashMap::new();
        expr_types.insert("#fn_caller.body.stmt_0".to_string(), Type::Unit);
        expr_types.insert("#fn_caller.body.stmt_1".to_string(), Type::Unit);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        let e0306 = errors
            .iter()
            .find(|e| e.code == E0306)
            .expect("expected E0306");
        assert!(
            e0306.message.contains("argument 0"),
            "E0306 message should pinpoint argument index: {}",
            e0306.message
        );
        assert!(
            e0306.message.contains("`wants_bool`"),
            "E0306 message should name the function: {}",
            e0306.message
        );
        // Expected/found ride the structured fields, not the message text —
        // the driver re-appends them, so embedding them too would double-print
        // (cf. E0307). Assert the fields carry the pair.
        assert_eq!(
            e0306.expected.as_deref(),
            Option::Some("bool"),
            "E0306 should carry expected in its structured field"
        );
        assert_eq!(
            e0306.found.as_deref(),
            Option::Some("i64"),
            "E0306 should carry found in its structured field"
        );
        assert!(
            !e0306.message.contains("expected `bool`"),
            "E0306 message must NOT embed expected/found (driver appends it): {}",
            e0306.message
        );
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
        let e0307 = errors
            .iter()
            .find(|e| e.code == E0307)
            .expect("expected E0307");
        assert!(
            e0307.message.contains("return type mismatch"),
            "E0307 should phrase the message clearly: {}",
            e0307.message
        );
        let fix = e0307.fix.as_deref().unwrap_or("");
        assert!(
            fix.contains("Ok("),
            "E0307 fix should suggest `Ok(...)` wrapping: {fix}"
        );
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
        let e0303 = errors
            .iter()
            .find(|e| e.code == E0303)
            .expect("expected E0303");
        assert!(
            e0303.message.contains("`?`"),
            "E0303 should mention the `?` operator: {}",
            e0303.message
        );
        assert!(
            e0303.message.contains("Result"),
            "E0303 should mention Result: {}",
            e0303.message
        );
        let fix = e0303.fix.as_deref().unwrap_or("");
        assert!(
            fix.contains("Result") || fix.contains("match"),
            "E0303 fix should guide the user: {fix}"
        );
    }

    #[test]
    fn e0301_message_names_inner_type() {
        // R01 via BinOp: Option<i32> + i32 — checker emits E0301 with the
        // wrapped type spelt out.
        let mut ctx = mk_ctx(HashMap::new());

        let program = make_program(vec![simple_fn(
            "f",
            vec![param(
                "x",
                AxonType::Option(Box::new(AxonType::Named("i32".into()))),
            )],
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
        expr_types.insert("#fn_f.body.stmt_0".to_string(), Type::I32);

        let errors = run_with_types(&mut ctx, &program, expr_types);
        let e0301 = errors
            .iter()
            .find(|e| e.code == E0301)
            .expect("expected E0301");
        assert!(
            e0301.message.contains("Option<i32>"),
            "E0301 should spell out Option<inner>: {}",
            e0301.message
        );
        let fix = e0301.fix.as_deref().unwrap_or("");
        assert!(
            fix.contains("unwrap_or") || fix.contains("match"),
            "E0301 fix should suggest unwrap or match: {fix}"
        );
    }

    #[test]
    fn span_propagates_to_e0305_when_stmt_has_span() {
        // When the enclosing Stmt carries a non-dummy span, the checker should
        // attach it to the diagnostic so lib.rs can render `file:line:col`.
        let mut sigs = HashMap::new();
        sigs.insert(
            "two_arg".to_string(),
            FnSig {
                params: vec![Type::I32, Type::I32],
                ret: Type::I32,
            },
        );
        let mut ctx = mk_ctx(sigs);

        // Build a block where the call statement has a real span.
        let call_expr = Expr::Call {
            callee: Box::new(ident("two_arg")),
            args: vec![lit_int(1)],
            tier: None,
        };
        let body = Expr::Block(vec![stmt_with_span(
            call_expr,
            crate::span::Span::new(15, 28),
        )]);

        let program = make_program(vec![simple_fn(
            "caller",
            vec![],
            Option::Some(AxonType::Named("i32".into())),
            body,
        )]);

        let errors = run(&mut ctx, &program);
        let e0305 = errors
            .iter()
            .find(|e| e.code == E0305)
            .expect("expected E0305");
        assert!(
            !e0305.span.is_dummy(),
            "E0305 should carry the statement's span (was dummy)"
        );
        assert_eq!(
            e0305.span.start, 15,
            "E0305 span should match the surrounding statement's start"
        );
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
            FnSig {
                params: vec![Type::Enum("Expr".into())],
                ret: Type::I64,
            },
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
                    tier: None,
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
