//! Single-pass name resolution for Axon.
//!
//! The resolver runs two sub-passes over a [`Program`]:
//!
//! 1. **Top-level collection** — every `FnDef`, `TypeDef`, `EnumDef`, and
//!    `ModDecl` name is entered into the global scope, together with all
//!    built-ins from [`crate::builtins::BUILTINS`].  Duplicate names produce
//!    [`E0002`].
//!
//! 2. **Body resolution** — each `FnDef` body is walked recursively.
//!    `Ident` nodes are looked up in the current scope chain; missing names
//!    produce [`E0001`] with a closest-match suggestion (Levenshtein ≤ 3).
//!    `Let`/`Own`/`RefBind` nodes define new [`Symbol::Local`]s in the
//!    innermost scope.  `Block` nodes push/pop a fresh scope.
//!
//! **Attribute validation** — deferred attributes (see
//! [`crate::builtins::DEFERRED_ATTRS`]) produce an [`I0001`] info diagnostic.
//! The well-known attributes `test` and `target` are silently accepted.
//! Anything else produces a warning (represented as a [`Severity::Warning`]
//! diagnostic).
//!
//! The resolver does **not** perform type-checking; that is left to `infer.rs`.

use std::collections::HashMap;

use crate::ast::{
    Expr, FmtPart, FnDef, Item, MatchArm, Pattern, Program, SelectArm, Stmt, UseDecl,
};
use crate::builtins::{BUILTINS, DEFERRED_ATTRS};
use crate::error::levenshtein;

// ── Diagnostic plumbing ───────────────────────────────────────────────────────
//
// `error.rs` is an empty stub written in parallel.  We define the diagnostic
// types we need here so that `resolver.rs` is self-contained.  When `error.rs`
// is complete these types will be replaced by imports from that module.

/// Diagnostic severity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

/// A compiler diagnostic emitted by the resolver.
#[derive(Debug, Clone)]
pub struct Diagnostic {
    /// Machine-readable error code (e.g. `"E0001"`).
    pub code: &'static str,
    /// Human-readable message.
    pub message: String,
    /// Source file name.
    pub file: String,
    /// 1-based line number (0 = unknown).
    pub line: u32,
    /// 1-based column number (0 = unknown).
    pub col: u32,
    /// Optional suggestion for how to fix the problem.
    pub fix: Option<String>,
    /// Severity level.
    pub severity: Severity,
    /// Byte-offset span for caret rendering.
    pub span: crate::span::Span,
}

impl Diagnostic {
    fn new(code: &'static str, message: impl Into<String>, severity: Severity) -> Self {
        Self {
            code,
            message: message.into(),
            file: String::new(),
            line: 0,
            col: 0,
            fix: None,
            severity,
            span: crate::span::Span::dummy(),
        }
    }

    fn with_span(mut self, span: crate::span::Span) -> Self {
        self.span = span;
        self
    }

    fn error(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(code, message, Severity::Error)
    }

    fn warning(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(code, message, Severity::Warning)
    }

    fn info(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(code, message, Severity::Info)
    }

    fn with_file(mut self, file: &str) -> Self {
        self.file = file.to_string();
        self
    }

    fn with_fix(mut self, fix: impl Into<String>) -> Self {
        self.fix = Some(fix.into());
        self
    }
}

// Error code constants matching the interface spec in error.rs.
const E0001: &str = "E0001"; // undefined name
const E0002: &str = "E0002"; // duplicate name
const E0003: &str = "E0003"; // module not found
// E0004 (item not exported) is Phase 2 — declared but not yet used.
#[allow(dead_code)]
const E0004: &str = "E0004";
const I0001: &str = "I0001"; // deferred attribute info

// ── Symbol ───────────────────────────────────────────────────────────────────

/// A resolved symbol entry.
#[derive(Debug, Clone)]
pub enum Symbol {
    /// A user-defined or built-in function.
    Fn {
        name: String,
        param_names: Vec<String>,
    },
    /// A struct/record type defined with `type`.
    Type { name: String },
    /// An enum type.
    Enum { name: String },
    /// A declared module.
    Mod { name: String },
    /// A local binding introduced by `let`, `own`, or `ref`.
    Local { name: String },
    /// A function that comes from the built-in table.
    Builtin { name: String },
}

impl Symbol {
    /// Return the bare name string for this symbol.
    ///
    /// Used by diagnostics and future passes (dead_code lint suppressed until
    /// those callers are written).
    #[allow(dead_code)]
    pub fn name(&self) -> &str {
        match self {
            Symbol::Fn { name, .. }
            | Symbol::Type { name }
            | Symbol::Enum { name }
            | Symbol::Mod { name }
            | Symbol::Local { name }
            | Symbol::Builtin { name } => name,
        }
    }
}

// ── SymbolTable ───────────────────────────────────────────────────────────────

/// A lexically-scoped symbol table.
///
/// Scopes are maintained as a stack of `HashMap`s.  Lookup walks from the
/// innermost scope outward, mirroring Axon's lexical scoping rules.
pub struct SymbolTable {
    scopes: Vec<HashMap<String, Symbol>>,
}

impl SymbolTable {
    /// Create a new table with a single (global) scope already pushed.
    pub fn new() -> Self {
        Self {
            scopes: vec![HashMap::new()],
        }
    }

    /// Push a fresh inner scope (e.g. entering a block or function body).
    pub fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    /// Pop the innermost scope.  Panics in debug builds if the global scope
    /// would be popped (that is a resolver bug, not a user error).
    pub fn pop_scope(&mut self) {
        debug_assert!(
            self.scopes.len() > 1,
            "resolver bug: attempted to pop the global scope"
        );
        self.scopes.pop();
    }

    /// Define `name` in the current (innermost) scope.
    ///
    /// Returns the previous symbol if one already existed **in the same scope**
    /// (shadowing an outer scope is not considered a duplicate).
    pub fn define(&mut self, name: String, sym: Symbol) -> Option<Symbol> {
        let scope = self
            .scopes
            .last_mut()
            .expect("resolver bug: define called with no scopes");
        scope.insert(name, sym)
    }

    /// Look up `name` walking from the innermost scope outward.
    pub fn lookup(&self, name: &str) -> Option<&Symbol> {
        for scope in self.scopes.iter().rev() {
            if let Some(sym) = scope.get(name) {
                return Some(sym);
            }
        }
        None
    }

    /// Current scope depth (0 = global, increases with each `push_scope`).
    pub fn depth(&self) -> usize {
        self.scopes.len()
    }

    /// Look up `name` and return the scope depth at which it was defined, or
    /// `None` if not found.  Depth 1 is the global scope.
    pub fn lookup_depth(&self, name: &str) -> Option<usize> {
        for (i, scope) in self.scopes.iter().enumerate().rev() {
            if scope.contains_key(name) {
                return Some(i + 1); // 1-based: 1 = global
            }
        }
        None
    }

    /// Return the name from the table that is closest (Levenshtein distance
    /// ≤ 3) to `name`, or `None` if no such name exists.
    ///
    /// Used to generate "did you mean …?" suggestions in E0001 diagnostics.
    pub fn suggest(&self, name: &str) -> Option<String> {
        let mut best: Option<(usize, &str)> = None;
        for scope in &self.scopes {
            for key in scope.keys() {
                let dist = levenshtein(name, key);
                if dist <= 3 {
                    match best {
                        None => best = Some((dist, key)),
                        Some((prev_dist, _)) if dist < prev_dist => {
                            best = Some((dist, key));
                        }
                        _ => {}
                    }
                }
            }
        }
        best.map(|(_, s)| s.to_string())
    }

    /// Iterate over every symbol name currently visible (all scopes).
    ///
    /// Useful for diagnostics and future IDE tooling (dead_code suppressed
    /// until those callers land).
    #[allow(dead_code)]
    pub fn all_visible_names(&self) -> impl Iterator<Item = &str> {
        self.scopes.iter().flat_map(|s| s.keys().map(String::as_str))
    }
}

impl Default for SymbolTable {
    fn default() -> Self {
        Self::new()
    }
}

// ── ResolveResult ─────────────────────────────────────────────────────────────

/// The complete output of the resolution pass.
pub struct ResolveResult {
    /// Symbol table after resolution (includes all top-level and local bindings
    /// that survived to the end of their scopes).
    pub table: SymbolTable,
    /// Error diagnostics.  All have `severity == Severity::Error`.
    pub errors: Vec<Diagnostic>,
    /// Warning diagnostics (e.g. W0001 for unknown attributes).
    pub warnings: Vec<Diagnostic>,
    /// Informational diagnostics (e.g. I0001 for deferred attributes).
    pub infos: Vec<Diagnostic>,
}

// ── Resolver ──────────────────────────────────────────────────────────────────

struct Resolver<'a> {
    file: &'a str,
    table: SymbolTable,
    errors: Vec<Diagnostic>,
    warnings: Vec<Diagnostic>,
    infos: Vec<Diagnostic>,
    /// Span of the statement currently being resolved.  The `Expr` enum has no
    /// per-variant span field, so we track the enclosing statement's span and
    /// attach it to undefined-name / shadowing diagnostics.
    current_span: crate::span::Span,
}

impl<'a> Resolver<'a> {
    fn new(file: &'a str) -> Self {
        Self {
            file,
            table: SymbolTable::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            infos: Vec::new(),
            current_span: crate::span::Span::dummy(),
        }
    }

    // ── Diagnostic helpers ────────────────────────────────────────────────

    fn emit_error(&mut self, d: Diagnostic) {
        self.errors.push(d);
    }

    fn emit_info(&mut self, d: Diagnostic) {
        self.infos.push(d);
    }

    fn emit_warning(&mut self, d: Diagnostic) {
        self.warnings.push(d);
    }

    // ── Pass 1: collect top-level names ──────────────────────────────────

    fn collect_top_level(&mut self, program: &Program) {
        // Seed builtins first so user-defined names can shadow them and so the
        // duplicate check distinguishes user↔builtin from user↔user collisions.
        for b in BUILTINS {
            let sym = Symbol::Builtin {
                name: b.name.to_string(),
            };
            // Builtins are pre-loaded; if a user redefines one we let it shadow
            // silently (the type-checker can warn later).
            self.table.define(b.name.to_string(), sym);
        }

        for item in &program.items {
            match item {
                Item::FnDef(f) => {
                    let sym = Symbol::Fn {
                        name: f.name.clone(),
                        param_names: f.params.iter().map(|p| p.name.clone()).collect(),
                    };
                    if let Some(prev) = self.table.define(f.name.clone(), sym) {
                        // Only flag true user↔user duplicates.
                        if !matches!(prev, Symbol::Builtin { .. }) {
                            self.emit_error(
                                Diagnostic::error(
                                    E0002,
                                    format!(
                                        "the name `{}` is defined more than once in this module",
                                        f.name
                                    ),
                                )
                                .with_file(self.file)
                                .with_span(f.span)
                                .with_fix(format!(
                                    "rename one of the `{}` definitions, or remove the duplicate",
                                    f.name
                                )),
                            );
                        }
                    }
                }
                Item::TypeDef(t) => {
                    let sym = Symbol::Type {
                        name: t.name.clone(),
                    };
                    if let Some(prev) = self.table.define(t.name.clone(), sym) {
                        if !matches!(prev, Symbol::Builtin { .. }) {
                            self.emit_error(
                                Diagnostic::error(
                                    E0002,
                                    format!(
                                        "the type `{}` is defined more than once in this module",
                                        t.name
                                    ),
                                )
                                .with_file(self.file)
                                .with_span(t.span)
                                .with_fix(format!(
                                    "rename one of the `{}` types, or remove the duplicate",
                                    t.name
                                )),
                            );
                        }
                    }
                }
                Item::EnumDef(e) => {
                    let sym = Symbol::Enum {
                        name: e.name.clone(),
                    };
                    if let Some(prev) = self.table.define(e.name.clone(), sym) {
                        if !matches!(prev, Symbol::Builtin { .. }) {
                            self.emit_error(
                                Diagnostic::error(
                                    E0002,
                                    format!(
                                        "the enum `{}` is defined more than once in this module",
                                        e.name
                                    ),
                                )
                                .with_file(self.file)
                                .with_span(e.span)
                                .with_fix(format!(
                                    "rename one of the `{}` enums, or remove the duplicate",
                                    e.name
                                )),
                            );
                        }
                    }
                    // Fix #11: register each variant name as a value-level symbol
                    // so that typos in patterns produce E0001.
                    for variant in &e.variants {
                        let variant_sym = Symbol::Enum {
                            name: format!("{}::{}", e.name, variant.name),
                        };
                        // Use the qualified name as the key so variant names don't
                        // shadow each other across enums; the resolver checks the
                        // qualified "EnumName::VariantName" form in patterns.
                        self.table.define(
                            format!("{}::{}", e.name, variant.name),
                            variant_sym,
                        );
                    }
                }
                Item::ModDecl(m) => {
                    let sym = Symbol::Mod {
                        name: m.name.clone(),
                    };
                    // Duplicate module declarations are an error.
                    if let Some(prev) = self.table.define(m.name.clone(), sym) {
                        if !matches!(prev, Symbol::Builtin { .. }) {
                            self.emit_error(
                                Diagnostic::error(
                                    E0002,
                                    format!(
                                        "module `{}` is declared more than once",
                                        m.name
                                    ),
                                )
                                .with_file(self.file),
                            );
                        }
                    }
                }
                Item::UseDecl(_) => {
                    // UseDecls are handled in pass 2 after all module names are known.
                }
                Item::TraitDef(_) | Item::ImplBlock(_) => {
                    // Phase 3: trait/impl blocks processed in a dedicated pass.
                }
                Item::LetDef { name, .. } => {
                    let sym = Symbol::Fn {
                        name: name.clone(),
                        param_names: vec![],
                    };
                    self.table.define(name.clone(), sym);
                }
            }
        }
    }

    // ── Pass 2: resolve bodies ────────────────────────────────────────────

    fn resolve_items(&mut self, program: &Program) {
        for item in &program.items {
            match item {
                Item::FnDef(f) => self.resolve_fn(f),
                Item::UseDecl(u) => self.resolve_use(u),
                Item::ImplBlock(blk) => {
                    for m in &blk.methods {
                        self.resolve_fn(m);
                    }
                }
                // TypeDef / EnumDef / ModDecl / TraitDef have no expressions to walk.
                Item::LetDef { value, .. } => self.resolve_expr(value),
                _ => {}
            }
        }
    }

    fn resolve_fn(&mut self, f: &FnDef) {
        // Validate attributes before entering the function body.
        self.validate_attrs(f);

        // Default the span to the function header so that any diagnostic raised
        // before we descend into a statement is at least pointed at the fn.
        if !f.span.is_dummy() {
            self.current_span = f.span;
        }

        self.table.push_scope();

        // Bring generic type parameters into scope as type symbols so that
        // type names like `T` inside the body don't produce E0001.
        for gp in &f.generic_params {
            self.table.define(gp.clone(), Symbol::Type { name: gp.clone() });
        }

        // Bring value parameters into scope.
        for p in &f.params {
            let sym = Symbol::Local {
                name: p.name.clone(),
            };
            self.table.define(p.name.clone(), sym);
        }

        self.resolve_expr(&f.body);

        self.table.pop_scope();
    }

    fn resolve_use(&mut self, u: &UseDecl) {
        // The first element of `path` must name a known module.
        if let Some(root) = u.path.first() {
            match self.table.lookup(root) {
                None => {
                    let suggestion = self.table.suggest(root);
                    let mut d = Diagnostic::error(
                        E0003,
                        format!("module `{root}` not found"),
                    )
                    .with_file(self.file);
                    if let Some(s) = suggestion {
                        d = d.with_fix(format!("did you mean `{s}`?"));
                    }
                    self.emit_error(d);
                }
                Some(sym) if !matches!(sym, Symbol::Mod { .. }) => {
                    self.emit_error(
                        Diagnostic::error(
                            E0003,
                            format!("`{root}` is not a module"),
                        )
                        .with_file(self.file),
                    );
                }
                Some(_) => {
                    // Module exists.  In Phase 1 we trust the item list;
                    // full validation (E0004) is Phase 2.
                }
            }
        }
    }

    // ── Attribute validation ──────────────────────────────────────────────

    fn validate_attrs(&mut self, f: &FnDef) {
        const KNOWN_ATTRS: &[&str] = &["test", "target", "inline", "allow", "derive"];

        for attr in &f.attrs {
            if DEFERRED_ATTRS.contains(&attr.name.as_str()) {
                self.emit_info(
                    Diagnostic::info(
                        I0001,
                        format!(
                            "`#[{}]` is a deferred Axon attribute; it is recognised by the \
                             runtime but not resolved by the Phase-1 compiler",
                            attr.name
                        ),
                    )
                    .with_file(self.file),
                );
            } else if !KNOWN_ATTRS.contains(&attr.name.as_str()) {
                self.emit_warning(
                    Diagnostic::warning(
                        "W0001",
                        format!("unknown attribute `#[{}]`", attr.name),
                    )
                    .with_file(self.file),
                );
            }
            // `test`, `target`, etc. — silently accepted.
        }
    }

    // ── Expression resolution ─────────────────────────────────────────────

    fn resolve_expr(&mut self, expr: &Expr) {
        match expr {
            // ── Block: new lexical scope ──────────────────────────────────
            Expr::Block(stmts) => {
                self.table.push_scope();
                for stmt in stmts {
                    self.resolve_stmt(stmt);
                }
                self.table.pop_scope();
            }

            // ── Bindings: define then resolve the value ───────────────────
            //
            // The value is resolved *before* the name is defined so that
            // `let x = x + 1` correctly reports `x` (the RHS) as undefined
            // when `x` hasn't been introduced yet.
            Expr::Let { name, value } => {
                self.resolve_expr(value);
                // Fix #15: warn when a new binding shadows an existing one.
                if self.table.lookup(name).is_some() {
                    self.emit_warning(
                        Diagnostic::warning(
                            "W0002",
                            format!("variable `{name}` shadows a previous binding"),
                        )
                        .with_file(self.file),
                    );
                }
                let sym = Symbol::Local { name: name.clone() };
                self.table.define(name.clone(), sym);
            }
            Expr::Own { name, value } => {
                self.resolve_expr(value);
                // Fix #15: warn on shadowing.
                if self.table.lookup(name).is_some() {
                    self.emit_warning(
                        Diagnostic::warning(
                            "W0002",
                            format!("variable `{name}` shadows a previous binding"),
                        )
                        .with_file(self.file),
                    );
                }
                let sym = Symbol::Local { name: name.clone() };
                self.table.define(name.clone(), sym);
            }
            Expr::RefBind { name, value } => {
                self.resolve_expr(value);
                // Fix #15: warn on shadowing.
                if self.table.lookup(name).is_some() {
                    self.emit_warning(
                        Diagnostic::warning(
                            "W0002",
                            format!("variable `{name}` shadows a previous binding"),
                        )
                        .with_file(self.file),
                    );
                }
                let sym = Symbol::Local { name: name.clone() };
                self.table.define(name.clone(), sym);
            }

            // ── Identifier lookup ─────────────────────────────────────────
            Expr::Ident(name) => {
                if self.table.lookup(name).is_none() {
                    let suggestion = self.table.suggest(name);
                    let mut d = Diagnostic::error(
                        E0001,
                        format!("cannot find name `{name}` in this scope"),
                    )
                    .with_file(self.file)
                    .with_span(self.current_span);
                    if let Some(s) = suggestion {
                        d = d.with_fix(format!(
                            "a name with a similar spelling exists — did you mean `{s}`?"
                        ));
                    } else {
                        d = d.with_fix(format!(
                            "introduce `{name}` with `let {name} = …`, or import it from a module"
                        ));
                    }
                    self.emit_error(d);
                }
            }

            // ── Call ──────────────────────────────────────────────────────
            Expr::Call { callee, args } => {
                self.resolve_expr(callee);
                for arg in args {
                    self.resolve_expr(arg);
                }
            }

            // ── Method call: resolve receiver + args; method name is
            //    deferred to type-checking since we need the receiver type ──
            Expr::MethodCall { receiver, args, .. } => {
                self.resolve_expr(receiver);
                for arg in args {
                    self.resolve_expr(arg);
                }
            }

            // ── Binary / unary operators ──────────────────────────────────
            Expr::BinOp { left, right, .. } => {
                self.resolve_expr(left);
                self.resolve_expr(right);
            }
            Expr::UnaryOp { operand, .. } => {
                self.resolve_expr(operand);
            }

            // ── ? operator ────────────────────────────────────────────────
            Expr::Question(inner) => {
                self.resolve_expr(inner);
            }

            // ── If ────────────────────────────────────────────────────────
            Expr::If { cond, then, else_ } => {
                self.resolve_expr(cond);
                self.resolve_expr(then);
                if let Some(e) = else_ {
                    self.resolve_expr(e);
                }
            }

            // ── Match ─────────────────────────────────────────────────────
            Expr::Match { subject, arms } => {
                self.resolve_expr(subject);
                for arm in arms {
                    self.resolve_arm(arm);
                }
            }

            // ── Spawn / Select ────────────────────────────────────────────
            Expr::Spawn(inner) => {
                self.resolve_expr(inner);
            }
            Expr::Select(arms) => {
                for arm in arms {
                    self.resolve_select_arm(arm);
                }
            }

            // ── Comptime ──────────────────────────────────────────────────
            Expr::Comptime(inner) => {
                self.resolve_expr(inner);
            }

            // ── Lambda ───────────────────────────────────────────────────
            Expr::Lambda { params, body, captures: _ } => {
                self.table.push_scope();
                for p in params {
                    let sym = Symbol::Local { name: p.name.clone() };
                    self.table.define(p.name.clone(), sym);
                }
                self.resolve_expr(body);
                self.table.pop_scope();
            }

            // ── Return ────────────────────────────────────────────────────
            Expr::Return(val) => {
                if let Some(v) = val {
                    self.resolve_expr(v);
                }
            }

            // ── Field access / Index ──────────────────────────────────────
            Expr::FieldAccess { receiver, .. } => {
                self.resolve_expr(receiver);
                // Field name validity is deferred to type-checking.
            }
            Expr::Index { receiver, index } => {
                self.resolve_expr(receiver);
                self.resolve_expr(index);
            }

            // ── Wrapper expressions ───────────────────────────────────────
            Expr::Ok(inner) | Expr::Err(inner) | Expr::Some(inner) => {
                self.resolve_expr(inner);
            }
            Expr::Array(elems) => {
                for e in elems {
                    self.resolve_expr(e);
                }
            }
            Expr::StructLit { fields, .. } => {
                for (_fname, fexpr) in fields {
                    self.resolve_expr(fexpr);
                }
            }

            // ── While ─────────────────────────────────────────────────────
            Expr::While { cond, body } => {
                self.resolve_expr(cond);
                self.table.push_scope();
                for stmt in body {
                    self.resolve_stmt(stmt);
                }
                self.table.pop_scope();
            }
            Expr::WhileLet { pattern, expr, body } => {
                self.resolve_expr(expr);
                self.table.push_scope();
                self.resolve_pattern(pattern);
                for stmt in body {
                    self.resolve_stmt(stmt);
                }
                self.table.pop_scope();
            }
            Expr::For { var, start, end, body, .. } => {
                self.resolve_expr(start);
                self.resolve_expr(end);
                self.table.push_scope();
                let sym = Symbol::Local { name: var.clone() };
                self.table.define(var.clone(), sym);
                for stmt in body {
                    self.resolve_stmt(stmt);
                }
                self.table.pop_scope();
            }

            // ── Assign (rebind existing local) ────────────────────────────
            Expr::Assign { name, value } => {
                self.resolve_expr(value);
                // The name must already be in scope.
                match self.table.lookup(name) {
                    None => {
                        let suggestion = self.table.suggest(name);
                        let mut d = Diagnostic::error(
                            E0001,
                            format!(
                                "cannot assign to `{name}` — no such variable is in scope"
                            ),
                        )
                        .with_file(self.file)
                        .with_span(self.current_span);
                        if let Some(s) = suggestion {
                            d = d.with_fix(format!(
                                "a similarly-named binding is in scope — did you mean `{s}`?"
                            ));
                        } else {
                            d = d.with_fix(format!(
                                "introduce `{name}` first with `let {name} = …`"
                            ));
                        }
                        self.emit_error(d);
                    }
                    // Fix #13: assigning to a function, type, or enum name is invalid.
                    Some(sym) if !matches!(sym, Symbol::Local { .. }) => {
                        let kind = match sym {
                            Symbol::Fn { .. } | Symbol::Builtin { .. } => "function",
                            Symbol::Type { .. } => "type",
                            Symbol::Enum { .. } => "enum",
                            Symbol::Mod { .. } => "module",
                            Symbol::Local { .. } => unreachable!(),
                        };
                        self.emit_error(
                            Diagnostic::error(
                                E0001,
                                format!(
                                    "cannot assign to {kind} name `{name}` — only mutable local \
                                     bindings can be reassigned"
                                ),
                            )
                            .with_file(self.file)
                            .with_span(self.current_span)
                            .with_fix("introduce a fresh `let` binding with a different name, \
                                       or rename the local you intended to assign to".to_string()),
                        );
                    }
                    Some(_) => {} // Symbol::Local — valid assignment target
                }
            }


            // ── FmtStr: resolve each interpolated sub-expression ─────────────
            Expr::FmtStr { parts } => {
                for part in parts {
                    if let FmtPart::Expr(e) = part {
                        self.resolve_expr(e);
                    }
                }
            }

            // ── Terminals: nothing to resolve ─────────────────────────────
            Expr::Literal(_) | Expr::None | Expr::Break | Expr::Continue => {}
        }
    }

    fn resolve_stmt(&mut self, stmt: &Stmt) {
        // Track the source span of the enclosing statement so that resolution
        // errors raised on its inner expressions carry a useful location.
        if !stmt.span.is_dummy() {
            self.current_span = stmt.span;
        }
        self.resolve_expr(&stmt.expr);
    }

    fn resolve_arm(&mut self, arm: &MatchArm) {
        // Each arm gets its own scope so pattern bindings don't escape.
        self.table.push_scope();
        self.resolve_pattern(&arm.pattern);
        if let Some(guard) = &arm.guard {
            self.resolve_expr(guard);
        }
        self.resolve_expr(&arm.body);
        self.table.pop_scope();
    }

    fn resolve_select_arm(&mut self, arm: &SelectArm) {
        self.resolve_expr(&arm.recv);
        self.resolve_expr(&arm.body);
    }

    /// Walk a pattern, defining any identifier bindings it introduces.
    fn resolve_pattern(&mut self, pat: &Pattern) {
        match pat {
            Pattern::Ident(name) => {
                // Pattern identifiers bind new locals (not lookups).
                let sym = Symbol::Local { name: name.clone() };
                self.table.define(name.clone(), sym);
            }
            Pattern::Some(inner)
            | Pattern::Ok(inner)
            | Pattern::Err(inner) => {
                self.resolve_pattern(inner);
            }
            Pattern::Struct { name, fields } => {
                // Fix #12: validate the struct/enum name is a known type symbol.
                // Patterns like `Point { x, y }` must refer to a known struct or
                // enum variant; `EnumName::VariantName { .. }` checks the qualified name.
                if self.table.lookup(name).is_none() {
                    let suggestion = self.table.suggest(name);
                    let mut d = Diagnostic::error(
                        E0001,
                        format!(
                            "cannot find type or enum variant `{name}` in this scope"
                        ),
                    )
                    .with_file(self.file)
                    .with_span(self.current_span);
                    if let Some(s) = suggestion {
                        d = d.with_fix(format!(
                            "a similarly-named type is in scope — did you mean `{s}`?"
                        ));
                    }
                    self.emit_error(d);
                }
                for (_, sub_pat) in fields {
                    self.resolve_pattern(sub_pat);
                }
            }
            Pattern::Tuple(pats) => {
                for p in pats {
                    self.resolve_pattern(p);
                }
            }
            // Wildcard, None, and Literal patterns introduce no bindings.
            Pattern::Wildcard | Pattern::None | Pattern::Literal(_) => {}
        }
    }
}

// ── Public entry-point ────────────────────────────────────────────────────────

/// Resolve all names in `program`.
///
/// `file` is the source file path used to annotate diagnostics.
///
/// The returned [`ResolveResult`] contains:
/// - The final [`SymbolTable`] (global scope + any remaining inner scopes,
///   though those will all have been popped by the end of a well-formed walk).
/// - `errors`: all [`Severity::Error`] and [`Severity::Warning`] diagnostics.
/// - `infos`: all [`Severity::Info`] diagnostics (e.g. I0001).
pub fn resolve_program(program: &Program, file: &str) -> ResolveResult {
    let mut r = Resolver::new(file);
    r.collect_top_level(program);
    r.resolve_items(program);

    ResolveResult {
        table: r.table,
        errors: r.errors,
        warnings: r.warnings,
        infos: r.infos,
    }
}

// ── Capture collection ────────────────────────────────────────────────────────

/// Walk every `Lambda` in `program` (mutably) and fill its `captures` field
/// with the set of variables referenced in the body that are not lambda params.
///
/// This is a post-resolution pass: names have already been validated, so we
/// can treat every `Ident` as a valid binding.  We track the outer-scope
/// locals using a simple `HashSet` stack.
pub fn fill_captures(program: &mut Program) {
    use std::collections::HashSet;
    for item in &mut program.items {
        match item {
            Item::FnDef(f) => {
                let outer: HashSet<String> =
                    f.params.iter().map(|p| p.name.clone()).collect();
                fill_captures_expr(&mut f.body, &outer);
            }
            Item::ImplBlock(blk) => {
                for m in &mut blk.methods {
                    let outer: HashSet<String> =
                        m.params.iter().map(|p| p.name.clone()).collect();
                    fill_captures_expr(&mut m.body, &outer);
                }
            }
            _ => {}
        }
    }
}

fn fill_captures_expr(expr: &mut Expr, outer: &std::collections::HashSet<String>) {
    use std::collections::HashSet;
    match expr {
        Expr::Lambda { params, body, captures } => {
            // The lambda's own params are NOT captures.
            let lambda_params: HashSet<String> = params.iter().map(|p| p.name.clone()).collect();

            // Collect all free variables in the body.
            let mut free: HashSet<String> = HashSet::new();
            collect_free_vars(body, &lambda_params, &mut free);

            // Captures = free vars that exist in the enclosing scope.
            *captures = free
                .into_iter()
                .filter(|n| outer.contains(n.as_str()))
                .map(|n| (n, None))
                .collect();
            captures.sort_by(|a, b| a.0.cmp(&b.0)); // deterministic order

            // Recurse into body with expanded outer scope.
            let mut inner_outer = outer.clone();
            inner_outer.extend(lambda_params);
            fill_captures_expr(body, &inner_outer);
        }
        // For all other expressions, just recurse.
        Expr::Block(stmts) => {
            let mut local_outer = outer.clone();
            for stmt in stmts {
                fill_captures_expr(&mut stmt.expr, &local_outer);
                // Let bindings extend the visible outer scope for subsequent stmts.
                if let Expr::Let { name, .. } = &stmt.expr {
                    local_outer.insert(name.clone());
                }
            }
        }
        Expr::Let { value, .. } | Expr::Own { value, .. } => fill_captures_expr(value, outer),
        Expr::RefBind { value, .. } => fill_captures_expr(value, outer),
        Expr::BinOp { left, right, .. } => {
            fill_captures_expr(left, outer);
            fill_captures_expr(right, outer);
        }
        Expr::UnaryOp { operand, .. } => fill_captures_expr(operand, outer),
        Expr::Call { callee, args } => {
            fill_captures_expr(callee, outer);
            for a in args {
                fill_captures_expr(a, outer);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            fill_captures_expr(receiver, outer);
            for a in args {
                fill_captures_expr(a, outer);
            }
        }
        Expr::If { cond, then, else_ } => {
            fill_captures_expr(cond, outer);
            fill_captures_expr(then, outer);
            if let Some(e) = else_ {
                fill_captures_expr(e, outer);
            }
        }
        Expr::Match { subject, arms } => {
            fill_captures_expr(subject, outer);
            for arm in arms {
                // Bind pattern names into arm scope.
                let mut arm_outer = outer.clone();
                collect_pattern_bindings(&arm.pattern, &mut arm_outer);
                fill_captures_expr(&mut arm.body, &arm_outer);
            }
        }
        Expr::Return(Some(e)) => fill_captures_expr(e, outer),
        Expr::Question(e) | Expr::Some(e) | Expr::Ok(e) | Expr::Err(e) => {
            fill_captures_expr(e, outer)
        }
        Expr::FieldAccess { receiver, .. } | Expr::Index { receiver, .. } => {
            fill_captures_expr(receiver, outer)
        }
        Expr::Assign { value, .. } => fill_captures_expr(value, outer),
        Expr::While { cond, body } => {
            fill_captures_expr(cond, outer);
            for stmt in body {
                fill_captures_expr(&mut stmt.expr, outer);
            }
        }
        Expr::WhileLet { expr, body, .. } => {
            fill_captures_expr(expr, outer);
            for stmt in body {
                fill_captures_expr(&mut stmt.expr, outer);
            }
        }
        Expr::For { start, end, body, .. } => {
            fill_captures_expr(start, outer);
            fill_captures_expr(end, outer);
            for stmt in body {
                fill_captures_expr(&mut stmt.expr, outer);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields {
                fill_captures_expr(v, outer);
            }
        }
        Expr::Array(elems) => {
            for e in elems {
                fill_captures_expr(e, outer);
            }
        }
        Expr::Spawn(e) | Expr::Comptime(e) => fill_captures_expr(e, outer),
        Expr::Select(arms) => {
            for arm in arms {
                fill_captures_expr(&mut arm.recv, outer);
                fill_captures_expr(&mut arm.body, outer);
            }
        }
        Expr::FmtStr { parts } => {
            for part in parts {
                if let crate::ast::FmtPart::Expr(e) = part {
                    fill_captures_expr(e, outer);
                }
            }
        }
        // Terminal nodes — no sub-expressions.
        Expr::Ident(_) | Expr::Literal(_) | Expr::None | Expr::Return(None)
        | Expr::Break | Expr::Continue => {}
    }
}

/// Collect all `Ident` names referenced in `expr` that are not bound by
/// `bound` (the lambda's own params).  Does NOT recurse into nested Lambdas
/// (those capture from their own outer scope, not this one).
fn collect_free_vars(
    expr: &Expr,
    bound: &std::collections::HashSet<String>,
    free: &mut std::collections::HashSet<String>,
) {
    match expr {
        Expr::Ident(name) => {
            if !bound.contains(name.as_str()) {
                free.insert(name.clone());
            }
        }
        // Stop at nested lambdas — they have their own capture analysis.
        Expr::Lambda { .. } => {}
        Expr::Block(stmts) => {
            let mut local_bound = bound.clone();
            for stmt in stmts {
                collect_free_vars(&stmt.expr, &local_bound, free);
                if let Expr::Let { name, .. } = &stmt.expr {
                    local_bound.insert(name.clone());
                }
            }
        }
        Expr::Let { value, .. } | Expr::Own { value, .. } => {
            collect_free_vars(value, bound, free)
        }
        Expr::RefBind { value, .. } => collect_free_vars(value, bound, free),
        Expr::BinOp { left, right, .. } => {
            collect_free_vars(left, bound, free);
            collect_free_vars(right, bound, free);
        }
        Expr::UnaryOp { operand, .. } => collect_free_vars(operand, bound, free),
        Expr::Call { callee, args } => {
            collect_free_vars(callee, bound, free);
            for a in args {
                collect_free_vars(a, bound, free);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_free_vars(receiver, bound, free);
            for a in args {
                collect_free_vars(a, bound, free);
            }
        }
        Expr::If { cond, then, else_ } => {
            collect_free_vars(cond, bound, free);
            collect_free_vars(then, bound, free);
            if let Some(e) = else_ {
                collect_free_vars(e, bound, free);
            }
        }
        Expr::Match { subject, arms } => {
            collect_free_vars(subject, bound, free);
            for arm in arms {
                let mut arm_bound = bound.clone();
                collect_pattern_bindings(&arm.pattern, &mut arm_bound);
                collect_free_vars(&arm.body, &arm_bound, free);
            }
        }
        Expr::Return(Some(e)) => collect_free_vars(e, bound, free),
        Expr::Question(e) | Expr::Some(e) | Expr::Ok(e) | Expr::Err(e) => {
            collect_free_vars(e, bound, free)
        }
        Expr::FieldAccess { receiver, .. } | Expr::Index { receiver, .. } => {
            collect_free_vars(receiver, bound, free)
        }
        Expr::Assign { value, .. } => collect_free_vars(value, bound, free),
        Expr::While { cond, body } => {
            collect_free_vars(cond, bound, free);
            for stmt in body {
                collect_free_vars(&stmt.expr, bound, free);
            }
        }
        Expr::WhileLet { expr, body, .. } => {
            collect_free_vars(expr, bound, free);
            for stmt in body {
                collect_free_vars(&stmt.expr, bound, free);
            }
        }
        Expr::For { start, end, body, var, .. } => {
            collect_free_vars(start, bound, free);
            collect_free_vars(end, bound, free);
            let mut for_bound = bound.clone();
            for_bound.insert(var.clone());
            for stmt in body {
                collect_free_vars(&stmt.expr, &for_bound, free);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_free_vars(v, bound, free);
            }
        }
        Expr::Array(elems) => {
            for e in elems {
                collect_free_vars(e, bound, free);
            }
        }
        Expr::Spawn(e) | Expr::Comptime(e) => collect_free_vars(e, bound, free),
        Expr::Select(arms) => {
            for arm in arms {
                collect_free_vars(&arm.recv, bound, free);
                collect_free_vars(&arm.body, bound, free);
            }
        }
        Expr::FmtStr { parts } => {
            for part in parts {
                if let crate::ast::FmtPart::Expr(e) = part {
                    collect_free_vars(e, bound, free);
                }
            }
        }
        Expr::Literal(_) | Expr::None | Expr::Return(None)
        | Expr::Break | Expr::Continue => {}
    }
}

fn collect_pattern_bindings(
    pat: &crate::ast::Pattern,
    bound: &mut std::collections::HashSet<String>,
) {
    use crate::ast::Pattern;
    match pat {
        Pattern::Ident(n) => {
            bound.insert(n.clone());
        }
        Pattern::Struct { fields, .. } => {
            for (_, sub) in fields {
                collect_pattern_bindings(sub, bound);
            }
        }
        Pattern::Tuple(pats) => {
            for p in pats {
                collect_pattern_bindings(p, bound);
            }
        }
        Pattern::Some(inner) | Pattern::Ok(inner) | Pattern::Err(inner) => {
            collect_pattern_bindings(inner, bound);
        }
        Pattern::Wildcard | Pattern::None | Pattern::Literal(_) => {}
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::*;

    // ── AST construction helpers ──────────────────────────────────────────────

    fn ident_expr(name: &str) -> Expr {
        Expr::Ident(name.to_string())
    }

    fn lit_int(n: i64) -> Expr {
        Expr::Literal(Literal::Int(n))
    }

    fn simple_fn(name: &str, body: Expr) -> Item {
        Item::FnDef(FnDef {
            public: false,
            name: name.to_string(),
            generic_params: vec![],
            generic_bounds: vec![],
            params: vec![],
            return_type: None,
            body,
            attrs: vec![],
            span: crate::span::Span::dummy(),
        })
    }

    fn fn_with_params(name: &str, params: Vec<(&str, &str)>, body: Expr) -> Item {
        Item::FnDef(FnDef {
            public: false,
            name: name.to_string(),
            generic_params: vec![],
            generic_bounds: vec![],
            params: params
                .into_iter()
                .map(|(n, ty)| Param {
                    name: n.to_string(),
                    ty: AxonType::Named(ty.to_string()),
                    span: crate::span::Span::dummy(),
                })
                .collect(),
            return_type: None,
            body,
            attrs: vec![],
            span: crate::span::Span::dummy(),
        })
    }

    fn fn_with_attrs(name: &str, attrs: Vec<&str>, body: Expr) -> Item {
        Item::FnDef(FnDef {
            public: false,
            name: name.to_string(),
            generic_params: vec![],
            generic_bounds: vec![],
            params: vec![],
            return_type: None,
            body,
            attrs: attrs
                .into_iter()
                .map(|a| Attr {
                    name: a.to_string(),
                    args: vec![],
                })
                .collect(),
            span: crate::span::Span::dummy(),
        })
    }

    fn program(items: Vec<Item>) -> Program {
        Program { items }
    }

    fn errors_with_code(result: &ResolveResult, code: &str) -> Vec<String> {
        result
            .errors
            .iter()
            .filter(|d| d.code == code)
            .map(|d| d.message.clone())
            .collect()
    }

    fn infos_with_code(result: &ResolveResult, code: &str) -> Vec<String> {
        result
            .infos
            .iter()
            .filter(|d| d.code == code)
            .map(|d| d.message.clone())
            .collect()
    }

    // ── E0001: undefined name ─────────────────────────────────────────────────

    #[test]
    fn undefined_name_produces_e0001() {
        let prog = program(vec![simple_fn("main", ident_expr("nonexistent"))]);
        let result = resolve_program(&prog, "test.ax");

        let e0001s = errors_with_code(&result, E0001);
        assert_eq!(e0001s.len(), 1, "expected exactly one E0001");
        assert!(
            e0001s[0].contains("nonexistent"),
            "message should mention the undefined name: {:?}",
            e0001s[0]
        );
    }

    #[test]
    fn undefined_name_e0001_includes_suggestion() {
        // "prntln" is close to "println" (built-in, distance 1).
        let prog = program(vec![simple_fn("main", ident_expr("prntln"))]);
        let result = resolve_program(&prog, "test.ax");

        let e0001s = errors_with_code(&result, E0001);
        assert_eq!(e0001s.len(), 1);

        // The fix field should suggest "println".
        let fixes: Vec<_> = result
            .errors
            .iter()
            .filter(|d| d.code == E0001)
            .filter_map(|d| d.fix.as_deref())
            .collect();
        assert!(
            fixes.iter().any(|f| f.contains("println")),
            "expected suggestion of `println`, got: {fixes:?}"
        );
    }

    // ── E0002: duplicate fn name ──────────────────────────────────────────────

    #[test]
    fn duplicate_fn_name_produces_e0002() {
        let prog = program(vec![
            simple_fn("greet", lit_int(1)),
            simple_fn("greet", lit_int(2)),
        ]);
        let result = resolve_program(&prog, "test.ax");

        let e0002s = errors_with_code(&result, E0002);
        assert_eq!(e0002s.len(), 1, "expected exactly one E0002");
        assert!(
            e0002s[0].contains("greet"),
            "message should mention the duplicated name"
        );
    }

    #[test]
    fn distinct_fn_names_produce_no_e0002() {
        let prog = program(vec![
            simple_fn("foo", lit_int(1)),
            simple_fn("bar", lit_int(2)),
        ]);
        let result = resolve_program(&prog, "test.ax");
        let e0002s = errors_with_code(&result, E0002);
        assert!(e0002s.is_empty(), "distinct names should not produce E0002");
    }

    // ── I0001: deferred attribute ─────────────────────────────────────────────

    #[test]
    fn deferred_attr_produces_i0001() {
        let prog = program(vec![fn_with_attrs("serve", vec!["agent"], lit_int(0))]);
        let result = resolve_program(&prog, "test.ax");

        let i0001s = infos_with_code(&result, I0001);
        assert_eq!(i0001s.len(), 1, "expected exactly one I0001");
        assert!(
            i0001s[0].contains("agent"),
            "info message should mention the attr name: {:?}",
            i0001s[0]
        );
    }

    #[test]
    fn all_deferred_attrs_produce_i0001() {
        // Smoke-test: every entry in DEFERRED_ATTRS triggers I0001.
        for attr_name in DEFERRED_ATTRS {
            let prog = program(vec![fn_with_attrs("f", vec![attr_name], lit_int(0))]);
            let result = resolve_program(&prog, "test.ax");
            let i0001s = infos_with_code(&result, I0001);
            assert_eq!(
                i0001s.len(),
                1,
                "attr `{attr_name}` should produce exactly one I0001, got {}",
                i0001s.len()
            );
        }
    }

    #[test]
    fn test_attr_does_not_produce_i0001() {
        let prog = program(vec![fn_with_attrs("my_test", vec!["test"], lit_int(0))]);
        let result = resolve_program(&prog, "test.ax");
        let i0001s = infos_with_code(&result, I0001);
        assert!(i0001s.is_empty(), "#[test] must not produce I0001");
    }

    // ── Built-in names resolve successfully ───────────────────────────────────

    #[test]
    fn builtin_names_resolve_successfully() {
        for b in BUILTINS {
            let prog = program(vec![simple_fn("main", ident_expr(b.name))]);
            let result = resolve_program(&prog, "test.ax");
            let e0001s = errors_with_code(&result, E0001);
            assert!(
                e0001s.is_empty(),
                "built-in `{}` should resolve without E0001; errors: {e0001s:?}",
                b.name
            );
        }
    }

    // ── Let binding scoping ───────────────────────────────────────────────────

    #[test]
    fn let_binding_available_after_definition() {
        // `let x = 1; x` — `x` is used after binding.
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Let {
                name: "x".to_string(),
                value: Box::new(lit_int(1)),
            }),
            Stmt::simple(ident_expr("x")),
        ]);
        let prog = program(vec![simple_fn("main", body)]);
        let result = resolve_program(&prog, "test.ax");
        let e0001s = errors_with_code(&result, E0001);
        assert!(
            e0001s.is_empty(),
            "`x` should resolve after `let x = 1`; errors: {e0001s:?}"
        );
    }

    #[test]
    fn let_binding_not_available_before_definition() {
        // `x; let x = 1` — `x` is used *before* the binding.
        let body = Expr::Block(vec![
            Stmt::simple(ident_expr("x")),
            Stmt::simple(Expr::Let {
                name: "x".to_string(),
                value: Box::new(lit_int(1)),
            }),
        ]);
        let prog = program(vec![simple_fn("main", body)]);
        let result = resolve_program(&prog, "test.ax");
        let e0001s = errors_with_code(&result, E0001);
        assert_eq!(
            e0001s.len(),
            1,
            "`x` should be undefined before its `let` binding"
        );
    }

    #[test]
    fn let_rhs_does_not_see_own_binding() {
        // `let x = x` — the RHS `x` should be undefined if nothing else defines it.
        let body = Expr::Let {
            name: "x".to_string(),
            value: Box::new(ident_expr("x")),
        };
        let prog = program(vec![simple_fn("main", body)]);
        let result = resolve_program(&prog, "test.ax");
        let e0001s = errors_with_code(&result, E0001);
        assert_eq!(
            e0001s.len(),
            1,
            "RHS of `let x = x` should see undefined `x`"
        );
    }

    // ── Parameters ────────────────────────────────────────────────────────────

    #[test]
    fn params_are_in_scope_in_body() {
        // fn add(a: i64, b: i64) -> i64 { a }
        let body = ident_expr("a");
        let prog = program(vec![fn_with_params("add", vec![("a", "i64"), ("b", "i64")], body)]);
        let result = resolve_program(&prog, "test.ax");
        let e0001s = errors_with_code(&result, E0001);
        assert!(
            e0001s.is_empty(),
            "function param `a` should be in scope in body; errors: {e0001s:?}"
        );
    }

    // ── Levenshtein helper ────────────────────────────────────────────────────

    #[test]
    fn levenshtein_exact_match_is_zero() {
        assert_eq!(levenshtein("println", "println"), 0);
    }

    #[test]
    fn levenshtein_single_insertion() {
        assert_eq!(levenshtein("prntln", "println"), 1);
    }

    #[test]
    fn levenshtein_completely_different_exceeds_cutoff() {
        // Distance between very different strings should be > 3.
        assert!(levenshtein("abc", "xyzwqr") > 3);
    }

    // ── E0003: module not found ───────────────────────────────────────────────

    #[test]
    fn use_unknown_module_produces_e0003() {
        let prog = program(vec![Item::UseDecl(UseDecl {
            path: vec!["nonexistent_mod".to_string()],
            items: vec!["Foo".to_string()],
        })]);
        let result = resolve_program(&prog, "test.ax");
        let e0003s = errors_with_code(&result, E0003);
        assert_eq!(e0003s.len(), 1, "expected E0003 for unknown module");
        assert!(e0003s[0].contains("nonexistent_mod"));
    }

    #[test]
    fn use_known_module_produces_no_e0003() {
        let prog = program(vec![
            Item::ModDecl(ModDecl {
                name: "server".to_string(),
            }),
            Item::UseDecl(UseDecl {
                path: vec!["server".to_string()],
                items: vec!["listen".to_string()],
            }),
        ]);
        let result = resolve_program(&prog, "test.ax");
        let e0003s = errors_with_code(&result, E0003);
        assert!(e0003s.is_empty(), "known module should not produce E0003");
    }

    // ── W0001: unknown attribute ──────────────────────────────────────────────

    fn warnings_with_code(result: &ResolveResult, code: &str) -> Vec<String> {
        result
            .warnings
            .iter()
            .filter(|d| d.code == code)
            .map(|d| d.message.clone())
            .collect()
    }

    #[test]
    fn unknown_attr_produces_w0001() {
        let prog = program(vec![fn_with_attrs("f", vec!["totally_unknown"], Expr::Block(vec![]))]);
        let result = resolve_program(&prog, "test.ax");
        let w0001s = warnings_with_code(&result, "W0001");
        assert!(!w0001s.is_empty(), "expected W0001 for unknown attribute, got: {:?}", result.warnings);
    }

    #[test]
    fn known_attr_test_produces_no_w0001() {
        let prog = program(vec![fn_with_attrs("my_test", vec!["test"], Expr::Block(vec![]))]);
        let result = resolve_program(&prog, "test.ax");
        let w0001s = warnings_with_code(&result, "W0001");
        assert!(w0001s.is_empty(), "#[test] should not produce W0001, got: {:?}", w0001s);
    }

    // ── W0002: variable shadowing ─────────────────────────────────────────────

    // ── Capture collection ────────────────────────────────────────────────────

    #[test]
    fn fill_captures_identifies_outer_variable() {
        // fn f() { let x = 1; let g = |y| x + y; }
        // The lambda `|y| x + y` captures `x` from the outer scope.
        let lambda = Expr::Lambda {
            params: vec![crate::ast::LambdaParam::untyped("y")],
            body: Box::new(Expr::BinOp {
                op: crate::ast::BinOp::Add,
                left: Box::new(Expr::Ident("x".into())),
                right: Box::new(Expr::Ident("y".into())),
            }),
            captures: vec![],
        };
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Let { name: "x".into(), value: Box::new(lit_int(1)) }),
            Stmt::simple(Expr::Let { name: "g".into(), value: Box::new(lambda) }),
        ]);
        let mut prog = program(vec![simple_fn("f", body)]);
        fill_captures(&mut prog);

        // Find the lambda in the AST and check captures.
        let fn_body = if let Item::FnDef(f) = &prog.items[0] { &f.body } else { panic!() };
        let g_stmt = if let Expr::Block(stmts) = fn_body { &stmts[1].expr } else { panic!() };
        let lambda = if let Expr::Let { value, .. } = g_stmt { value.as_ref() } else { panic!() };
        if let Expr::Lambda { captures, .. } = lambda {
            let names: Vec<&str> = captures.iter().map(|(n, _)| n.as_str()).collect();
            assert_eq!(names, ["x"], "expected capture [x], got {:?}", names);
        } else {
            panic!("expected Lambda");
        }
    }

    #[test]
    fn fill_captures_does_not_capture_own_params() {
        // |x| x + 1 — x is a param, not a capture.
        let lambda = Expr::Lambda {
            params: vec![crate::ast::LambdaParam::untyped("x")],
            body: Box::new(Expr::BinOp {
                op: crate::ast::BinOp::Add,
                left: Box::new(Expr::Ident("x".into())),
                right: Box::new(lit_int(1)),
            }),
            captures: vec![],
        };
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Let { name: "g".into(), value: Box::new(lambda) }),
        ]);
        let mut prog = program(vec![simple_fn("f", body)]);
        fill_captures(&mut prog);

        let fn_body = if let Item::FnDef(f) = &prog.items[0] { &f.body } else { panic!() };
        let g_stmt = if let Expr::Block(stmts) = fn_body { &stmts[0].expr } else { panic!() };
        let lambda = if let Expr::Let { value, .. } = g_stmt { value.as_ref() } else { panic!() };
        if let Expr::Lambda { captures, .. } = lambda {
            assert!(captures.is_empty(), "own params should not be captured, got {:?}", captures);
        } else {
            panic!("expected Lambda");
        }
    }

    #[test]
    fn shadowing_let_produces_w0002() {
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Let { name: "x".into(), value: Box::new(lit_int(1)) }),
            Stmt::simple(Expr::Let { name: "x".into(), value: Box::new(lit_int(2)) }),
        ]);
        let prog = program(vec![simple_fn("f", body)]);
        let result = resolve_program(&prog, "test.ax");
        let w0002s = warnings_with_code(&result, "W0002");
        assert!(!w0002s.is_empty(), "expected W0002 for variable shadowing, got: {:?}", result.warnings);
    }
}
