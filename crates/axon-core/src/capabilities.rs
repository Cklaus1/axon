//! Capability permission checker — Phase 4: `@[contained(...)]`
//!
//! For each `FnDef` that has a `ContainedSpec`, this pass walks the function
//! body looking for I/O call sites and validates them against the spec.
//!
//! ## Error codes
//! E1001 — I/O call not permitted by `@[contained]` spec (path/host outside allowlist)
//! E1002 — `@[contained]` clause is malformed (currently unused; reserved for future validation)
//! E1003 — capability path is not parseable
//! E1004 — call hits a `never:` clause (hard violation, even if allowlist would permit it)

use crate::ast::{ContainedSpec, Expr, FnDef, Item, NeverClause, Program, Stmt};
use crate::span::Span;

// ── Error codes ───────────────────────────────────────────────────────────────

pub const E1001: &str = "E1001";
pub const E1002: &str = "E1002";
pub const E1003: &str = "E1003";
pub const E1004: &str = "E1004";
pub const E1203: &str = "E1203"; // import widens the importer's capability surface (R6 §4.4)

// ── Diagnostic ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct CapabilityError {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

impl CapabilityError {
    fn new(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self { code, message: message.into(), span }
    }
}

// ── I/O call classification ───────────────────────────────────────────────────

/// The kind of I/O operation a builtin call represents.
#[derive(Debug)]
enum IoKind {
    FsRead,
    FsWrite,
    Net,
    // Part of the @[contained] effect taxonomy used by the codegen path; not yet
    // produced by `classify_call` here, but kept so the enum stays exhaustive.
    #[allow(dead_code)]
    Exec,
}

/// Match a function name to an I/O kind.
fn classify_call(name: &str) -> Option<IoKind> {
    match name {
        "read_file"                             => Some(IoKind::FsRead),
        "write_file"                            => Some(IoKind::FsWrite),
        "exec"                                  => Some(IoKind::Exec),
        // Future net calls (http_get, ai_complete, etc.) — treat as net
        "http_get"
        | "http_post"
        | "ai_complete"
        | "ai_extract_uncertain_i64"
        | "ai_extract_uncertain_f64" => Some(IoKind::Net),
        // Generic Layer-3 ASI surface: `ai_extract::<T>(prompt)` lowers to a
        // `Call { callee: StructLit { name: "ai_extract::<T>", … }, … }`.
        // Every concrete T is a network call regardless of the v1 dispatch set.
        n if n.starts_with("ai_extract::<") => Some(IoKind::Net),
        _ => None,
    }
}

// ── Path / host matching ──────────────────────────────────────────────────────

/// Returns true if `path` has `prefix` as a path prefix.
/// e.g. `path_has_prefix("./data/x.txt", "./data/")` → true
///
/// SECURITY: a path containing a `..` component never matches — otherwise
/// `"./out/../etc/passwd"` would pass the raw `starts_with("./out/")` test and
/// escape the sandbox via traversal. A `..` path can't be statically proven to
/// stay within the allowed prefix, so the capability check denies it (the call
/// is refused, the conservative-safe outcome). Paths without `..` use the plain
/// prefix test.
fn path_has_prefix(path: &str, prefix: &str) -> bool {
    if path_has_dotdot(path) {
        return false;
    }
    path.starts_with(prefix)
}

/// True if `path` contains a `..` path component (`..`, `../`, `/..`, `/../`).
/// A bare `..` inside a filename (e.g. `a..b`) is not a traversal component.
fn path_has_dotdot(path: &str) -> bool {
    path.split('/').any(|seg| seg == "..")
}

/// Check if `host` matches a glob pattern like `*.myapi.com`.
/// Only supports leading `*` wildcard for now.
fn host_matches_glob(host: &str, glob: &str) -> bool {
    if let Some(suffix) = glob.strip_prefix('*') {
        host.ends_with(suffix)
    } else {
        host == glob
    }
}

/// The directory prefix of a file path, used to suggest a minimal allowlist
/// entry. `"./data/x.txt"` → `"./data/"`; a bare filename → the path itself.
/// Bug #8: capability errors should suggest the exact clause to add, and the
/// directory prefix is the least-privilege grant that would permit the call.
fn dir_prefix(path: &str) -> &str {
    match path.rfind('/') {
        Some(i) => &path[..=i], // include the trailing slash
        None => path,
    }
}

// ── Core check ───────────────────────────────────────────────────────────────

/// Run the capability check on all functions in `program` and return diagnostics.
pub fn check_capabilities(program: &Program) -> Vec<CapabilityError> {
    let mut errors = Vec::new();
    // Map fn-name → FnDef so a `@[contained]` fn's capability check can follow
    // calls into user helpers transitively (a sandbox must not be escapable by
    // moving the forbidden I/O one function call away).
    let fn_map: std::collections::HashMap<&str, &FnDef> = program
        .items
        .iter()
        .filter_map(|item| match item {
            Item::FnDef(f) => Some((f.name.as_str(), f)),
            _ => None,
        })
        .collect();
    for item in &program.items {
        if let Item::FnDef(fndef) = item {
            check_fn(fndef, &fn_map, &mut errors);
        }
    }
    errors
}

fn check_fn<'a>(
    fndef: &'a FnDef,
    fn_map: &std::collections::HashMap<&'a str, &'a FnDef>,
    errors: &mut Vec<CapabilityError>,
) {
    let spec = match &fndef.contained {
        Some(s) => s,
        None => return, // no @[contained] — no restrictions
    };
    // Walk the function body AND every user helper it transitively reaches,
    // checking each against this fn's spec. `visited` starts with the contained
    // fn itself so a self/mutual recursion can't loop.
    let mut visited: std::collections::HashSet<&str> = std::collections::HashSet::new();
    visited.insert(fndef.name.as_str());
    let mut ctx = CapCtx { spec, fn_map, visited, errors };
    check_expr(&fndef.body, &mut ctx);
}

/// Threaded state for the transitive `@[contained]` walk: the spec being
/// enforced, the program's fn map (to follow helper calls), the visited set (to
/// stop recursion), and the error sink.
struct CapCtx<'a, 'e> {
    spec: &'a ContainedSpec,
    fn_map: &'a std::collections::HashMap<&'a str, &'a FnDef>,
    visited: std::collections::HashSet<&'a str>,
    errors: &'e mut Vec<CapabilityError>,
}

/// The set of capability *kinds* a program exercises — `"fs:read"`,
/// `"fs:write"`, `"net"`, `"exec"` — collected from every call site regardless
/// of `@[contained]`. This is the canonical capability surface used by R10's
/// G2 safety gate: a candidate compiler pass `P` is unsafe iff
/// `program_capabilities(P(c))` is NOT a subset of `program_capabilities(c)`,
/// i.e. the transform introduced a capability the original program lacked
/// (I-12 — self-modification cannot widen the trusted surface). Reuses
/// `classify_call` so the capability taxonomy stays single-sourced.
pub fn program_capabilities(program: &Program) -> std::collections::BTreeSet<String> {
    let mut caps = std::collections::BTreeSet::new();
    for item in &program.items {
        if let Item::FnDef(f) = item {
            collect_caps_expr(&f.body, &mut caps);
        }
    }
    caps
}

/// The capability kind a builtin call exercises (`"fs:read"`, `"fs:write"`,
/// `"net"`, `"exec"`), or `None` for a pure builtin. Single-sourced on
/// `classify_call` so the taxonomy is shared by the `@[contained]` checker, the
/// R10 capability-diff, and R4's `@[agent]` action log (which records the
/// `caps_used` of each agent action). Public so the interpreter can stamp it.
pub fn capability_of_builtin(name: &str) -> Option<&'static str> {
    classify_call(name).map(|k| cap_label(&k))
}

/// The capability *ceiling* an importer's `@[contained]` declarations grant —
/// the union of capability kinds any contained fn in the program is allowed to
/// exercise. Used as the boundary the import-edge check enforces (R6 §4.4).
///
/// An importer that declares **no** `@[contained]` has no ceiling (`None`):
/// it is uncontained, so importing a capability-exercising module is not a
/// *widening* — there was no declared boundary to widen. This keeps E1203
/// opt-in: a program only gains import-edge protection once it declares its own
/// containment, so existing uncontained programs are unaffected (back-compat).
fn importer_grant_ceiling(importer: &Program) -> Option<std::collections::BTreeSet<String>> {
    let mut ceiling = std::collections::BTreeSet::new();
    let mut declared = false;
    for item in &importer.items {
        if let Item::FnDef(f) = item {
            if let Some(spec) = &f.contained {
                declared = true;
                if !spec.fs_read.is_empty() {
                    ceiling.insert("fs:read".to_string());
                }
                if !spec.fs_write.is_empty() {
                    ceiling.insert("fs:write".to_string());
                }
                if !spec.net_allow.is_empty() {
                    ceiling.insert("net".to_string());
                }
                if spec.exec_allowed {
                    ceiling.insert("exec".to_string());
                }
            }
        }
    }
    if declared {
        Some(ceiling)
    } else {
        None
    }
}

/// R6 §4.4 — the import-edge capability check (E1203, the import-edge extension
/// of I-11). An imported module may not *widen* the importer's declared
/// capability surface: if the importer is `@[contained]` and an imported module
/// **exercises** a capability kind (fs:read/fs:write/net/exec) the importer does
/// not grant, that is **E1203**.
///
/// The importer's grant is the union of its `@[contained]` allowlists
/// ([`importer_grant_ceiling`]); the import's demand is the capabilities its
/// code actually calls ([`program_capabilities`]). An *uncontained* importer
/// has no ceiling, so nothing widens — E1203 is opt-in, gained only when a
/// program declares its own containment.
///
/// This is a **TCB-grade boundary** (I-11): the static capability checker is the
/// hard gate; an AI import-audit (R6 §4.3) is defense-in-depth layered above it,
/// never a substitute. Returns one `CapabilityError` per excess capability.
pub fn check_import_capabilities(
    importer: &Program,
    import_name: &str,
    imported: &Program,
) -> Vec<CapabilityError> {
    let Some(ceiling) = importer_grant_ceiling(importer) else {
        // Uncontained importer — no declared boundary, nothing to widen.
        return Vec::new();
    };
    let demanded = program_capabilities(imported);
    let mut errors = Vec::new();
    for cap in demanded.difference(&ceiling) {
        errors.push(CapabilityError::new(
            E1203,
            format!(
                "import `{import_name}` exercises capability `{cap}`, which the importing \
                 @[contained] program does not grant — widen the importer's @[contained] \
                 to permit `{cap}`, or remove the import (R6 §4.4, import-edge boundary)"
            ),
            Span::dummy(),
        ));
    }
    // Deterministic order (BTreeSet difference is already sorted, but be explicit).
    errors
}

fn cap_label(kind: &IoKind) -> &'static str {
    match kind {
        IoKind::FsRead => "fs:read",
        IoKind::FsWrite => "fs:write",
        IoKind::Net => "net",
        IoKind::Exec => "exec",
    }
}

fn collect_caps_stmts(stmts: &[Stmt], caps: &mut std::collections::BTreeSet<String>) {
    for stmt in stmts {
        collect_caps_expr(&stmt.expr, caps);
    }
}

/// Walk an expression, recording the capability kind of every call site. Mirrors
/// `check_expr`'s traversal but capability-collecting instead of spec-checking.
fn collect_caps_expr(expr: &Expr, caps: &mut std::collections::BTreeSet<String>) {
    match expr {
        Expr::Call { callee, args, .. } => {
            let name = match callee.as_ref() {
                Expr::Ident(n) => Some(n.as_str()),
                Expr::StructLit { name, fields } if fields.is_empty() => Some(name.as_str()),
                _ => None,
            };
            if let Some(n) = name {
                if let Some(kind) = classify_call(n) {
                    caps.insert(cap_label(&kind).to_string());
                }
            }
            collect_caps_expr(callee, caps);
            for arg in args {
                collect_caps_expr(arg, caps);
            }
        }
        Expr::Block(stmts) => collect_caps_stmts(stmts, caps),
        Expr::Let { value, .. } | Expr::Own { value, .. } | Expr::RefBind { value, .. } => {
            collect_caps_expr(value, caps)
        }
        Expr::BinOp { left, right, .. } => {
            collect_caps_expr(left, caps);
            collect_caps_expr(right, caps);
        }
        Expr::UnaryOp { operand, .. } => collect_caps_expr(operand, caps),
        Expr::Question(inner) => collect_caps_expr(inner, caps),
        Expr::MethodCall { receiver, args, .. } => {
            collect_caps_expr(receiver, caps);
            for arg in args {
                collect_caps_expr(arg, caps);
            }
        }
        Expr::If { cond, then, else_ } => {
            collect_caps_expr(cond, caps);
            collect_caps_expr(then, caps);
            if let Some(e) = else_ {
                collect_caps_expr(e, caps);
            }
        }
        Expr::Match { subject, arms } => {
            collect_caps_expr(subject, caps);
            for arm in arms {
                if let Some(g) = &arm.guard {
                    collect_caps_expr(g, caps);
                }
                collect_caps_expr(&arm.body, caps);
            }
        }
        Expr::While { cond, body } => {
            collect_caps_expr(cond, caps);
            collect_caps_stmts(body, caps);
        }
        Expr::WhileLet { expr, body, .. } => {
            collect_caps_expr(expr, caps);
            collect_caps_stmts(body, caps);
        }
        Expr::For { start, end, body, .. } => {
            collect_caps_expr(start, caps);
            collect_caps_expr(end, caps);
            collect_caps_stmts(body, caps);
        }
        Expr::Assign { value, .. } => collect_caps_expr(value, caps),
        Expr::AssignTo { place, value } => {
            collect_caps_expr(place, caps);
            collect_caps_expr(value, caps);
        }
        Expr::Return(Some(e)) => collect_caps_expr(e, caps),
        Expr::FieldAccess { receiver, .. } => collect_caps_expr(receiver, caps),
        Expr::Index { receiver, index } => {
            collect_caps_expr(receiver, caps);
            collect_caps_expr(index, caps);
        }
        Expr::Ok(inner) | Expr::Err(inner) | Expr::Some(inner) => collect_caps_expr(inner, caps),
        Expr::Array(elems) | Expr::Tuple(elems) => {
            for e in elems {
                collect_caps_expr(e, caps);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields {
                collect_caps_expr(v, caps);
            }
        }
        Expr::Lambda { body, .. } => collect_caps_expr(body, caps),
        Expr::FmtStr { parts } => {
            for part in parts {
                if let crate::ast::FmtPart::Expr(e) = part {
                    collect_caps_expr(e, caps);
                }
            }
        }
        _ => {}
    }
}

fn check_stmts<'a>(stmts: &'a [Stmt], ctx: &mut CapCtx<'a, '_>) {
    for stmt in stmts {
        check_expr(&stmt.expr, ctx);
    }
}

fn check_expr<'a>(expr: &'a Expr, ctx: &mut CapCtx<'a, '_>) {
    match expr {
        Expr::Call { callee, args, .. } => {
            // Plain-ident call sites:    `ai_extract_uncertain_i64(p)`
            // Generic-builtin call sites: `ai_extract::<T>(p)` lowers to
            //   `Call { callee: StructLit { name: "ai_extract::<T>", … } }`
            // — see `parser.rs::parse_ai_extract_turbofish`.  Both shapes
            // funnel through `classify_call`, which keys on the synthetic name.
            let callee_name = match callee.as_ref() {
                Expr::Ident(name) => Some(name.as_str()),
                Expr::StructLit { name, fields } if fields.is_empty() => Some(name.as_str()),
                _ => None,
            };
            if let Some(name) = callee_name {
                // A builtin I/O call is checked against the spec directly.
                check_call(name, args, ctx.spec, ctx.errors);
                // A USER fn call escapes the sandbox unless we follow it: the
                // helper's body must also satisfy this spec (the helper inherits
                // the caller's containment). Guard against recursion via `visited`.
                if classify_call(name).is_none() {
                    if let Some(helper) = ctx.fn_map.get(name) {
                        if ctx.visited.insert(name) {
                            // A helper with its OWN @[contained] is checked under
                            // its own spec elsewhere; still descend so a stricter
                            // CALLER spec also constrains it (defense in depth).
                            check_expr(&helper.body, ctx);
                            ctx.visited.remove(name);
                        }
                    }
                }
            }
            // Recurse into callee and args.
            check_expr(callee, ctx);
            for arg in args {
                check_expr(arg, ctx);
            }
        }

        // Recurse into all compound expressions.
        Expr::Block(stmts) => check_stmts(stmts, ctx),
        // Phase 6: capability checks must see through a `with` block — both the
        // handler arm bodies and the wrapped body can make capability-relevant
        // calls.
        Expr::WithHandler { handler, body } => {
            if let crate::ast::HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                for arm in arms.iter().chain(return_arm.as_deref()) {
                    check_expr(&arm.body, ctx);
                }
            }
            check_expr(body, ctx);
        }
        Expr::Let { value, .. }
        | Expr::Own { value, .. }
        | Expr::RefBind { value, .. } => check_expr(value, ctx),
        Expr::BinOp { left, right, .. } => {
            check_expr(left, ctx);
            check_expr(right, ctx);
        }
        Expr::UnaryOp { operand, .. } => check_expr(operand, ctx),
        Expr::Question(inner) => check_expr(inner, ctx),
        Expr::MethodCall { receiver, args, .. } => {
            check_expr(receiver, ctx);
            for arg in args {
                check_expr(arg, ctx);
            }
        }
        Expr::If { cond, then, else_ } => {
            check_expr(cond, ctx);
            check_expr(then, ctx);
            if let Some(e) = else_ {
                check_expr(e, ctx);
            }
        }
        Expr::Match { subject, arms } => {
            check_expr(subject, ctx);
            for arm in arms {
                if let Some(g) = &arm.guard { check_expr(g, ctx); }
                check_expr(&arm.body, ctx);
            }
        }
        Expr::While { cond, body } => {
            check_expr(cond, ctx);
            check_stmts(body, ctx);
        }
        Expr::WhileLet { expr, body, .. } => {
            check_expr(expr, ctx);
            check_stmts(body, ctx);
        }
        Expr::For { start, end, body, .. } => {
            check_expr(start, ctx);
            check_expr(end, ctx);
            check_stmts(body, ctx);
        }
        Expr::Assign { value, .. } => check_expr(value, ctx),
        Expr::AssignTo { place, value } => {
            check_expr(place, ctx);
            check_expr(value, ctx);
        }
        Expr::Return(inner) => {
            if let Some(e) = inner { check_expr(e, ctx); }
        }
        Expr::FieldAccess { receiver, .. } => check_expr(receiver, ctx),
        Expr::Index { receiver, index } => {
            check_expr(receiver, ctx);
            check_expr(index, ctx);
        }
        Expr::Ok(inner) | Expr::Err(inner) | Expr::Some(inner) => {
            check_expr(inner, ctx);
        }
        Expr::Array(elems) | Expr::Tuple(elems) => {
            for e in elems { check_expr(e, ctx); }
        }
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields { check_expr(v, ctx); }
        }
        Expr::FmtStr { parts } => {
            for part in parts {
                if let crate::ast::FmtPart::Expr(e) = part {
                    check_expr(e, ctx);
                }
            }
        }
        Expr::Lambda { body, .. } => check_expr(body, ctx),
        Expr::Spawn(body) => check_expr(body, ctx),
        Expr::Select(arms) => {
            for arm in arms {
                check_expr(&arm.recv, ctx);
                check_expr(&arm.body, ctx);
            }
        }
        Expr::Comptime(inner) => check_expr(inner, ctx),
        // Leaf nodes — no recursion needed.
        Expr::Ident(_) | Expr::Literal(_) | Expr::None | Expr::Break | Expr::Continue => {}
    }
}

/// Validate a single I/O call against the spec.
fn check_call(
    name: &str,
    args: &[Expr],
    spec: &ContainedSpec,
    errors: &mut Vec<CapabilityError>,
) {
    let kind = match classify_call(name) {
        Some(k) => k,
        None => return, // not an I/O builtin
    };

    // Extract a literal string argument (first arg for read_file/write_file/http
    // calls).
    //
    // KNOWN LIMITATION (v1, intentional — see `non_literal_arg_is_skipped`): the
    // capability check only applies to a LITERAL target. A computed target —
    // `let h = …; ai_complete(h)`, `read_file(some_path_var)` — is `None` here
    // and the allowlist/never check below is SKIPPED, so it is not statically
    // flagged. This is a deliberate static-analysis boundary: the checker can't
    // know a computed value, and denying all dynamic targets would forbid
    // legitimate code. It is therefore a real residual sandbox gap for
    // dynamically-constructed targets — closed only at runtime (the Phase-9
    // `Sandbox<P>` runtime enforcement, ROADMAP). Literal targets ARE precisely
    // checked, including `..` traversal (see `path_has_prefix`).
    let literal_arg: Option<&str> = args.first().and_then(|a| {
        if let Expr::Literal(crate::ast::Literal::Str(s)) = a {
            Some(s.as_str())
        } else {
            None
        }
    });

    match &kind {
        IoKind::FsRead => {
            if let Some(path) = literal_arg {
                let pfx = dir_prefix(path);
                // 1. Check never: rules first (hard violation).
                for clause in &spec.never {
                    if let NeverClause::Read(prefix) = clause {
                        if path_has_prefix(path, prefix) {
                            errors.push(CapabilityError::new(
                                E1004,
                                format!(
                                    "`read_file(\"{path}\")` is forbidden by `never: [read(\"{prefix}\")]`\n  \
                                     help: remove the `never: [read(\"{prefix}\")]` clause, or remove the read call — a `never` rule is a hard deny that no allowlist can override"
                                ),
                                Span::dummy(),
                            ));
                            return;
                        }
                    }
                }
                // 2. Check allowlist.
                if spec.fs_read.is_empty() {
                    // No fs read allowlist — deny all reads.
                    errors.push(CapabilityError::new(
                        E1001,
                        format!(
                            "`read_file(\"{path}\")` is not permitted: no `fs: [read(...)]` in @[contained]\n  \
                             help: Add `fs: [read(\"{pfx}\")]` to the @[contained(...)] attribute to allow this read"
                        ),
                        Span::dummy(),
                    ));
                } else if !spec.fs_read.iter().any(|p| path_has_prefix(path, p)) {
                    errors.push(CapabilityError::new(
                        E1001,
                        format!(
                            "`read_file(\"{path}\")` is not permitted by @[contained] \
                             (allowed prefixes: {})\n  \
                             help: Add `read(\"{pfx}\")` to the existing `fs: [...]` clause",
                            spec.fs_read.iter().map(|p| format!("\"{p}\"")).collect::<Vec<_>>().join(", ")
                        ),
                        Span::dummy(),
                    ));
                }
            } else {
                // Non-literal path — emit info (runtime enforcement needed).
                // We do not emit an error for dynamic paths; the spec says skip.
            }
        }

        IoKind::FsWrite => {
            if let Some(path) = literal_arg {
                let pfx = dir_prefix(path);
                // 1. never: write check.
                for clause in &spec.never {
                    if let NeverClause::Write(prefix) = clause {
                        if path_has_prefix(path, prefix) {
                            errors.push(CapabilityError::new(
                                E1004,
                                format!(
                                    "`write_file(\"{path}\", ...)` is forbidden by `never: [write(\"{prefix}\")]`\n  \
                                     help: remove the `never: [write(\"{prefix}\")]` clause, or remove the write call — a `never` rule is a hard deny that no allowlist can override"
                                ),
                                Span::dummy(),
                            ));
                            return;
                        }
                    }
                }
                // 2. Allowlist check.
                if spec.fs_write.is_empty() {
                    errors.push(CapabilityError::new(
                        E1001,
                        format!(
                            "`write_file(\"{path}\", ...)` is not permitted: no `fs: [write(...)]` in @[contained]\n  \
                             help: Add `fs: [write(\"{pfx}\")]` to the @[contained(...)] attribute to allow this write"
                        ),
                        Span::dummy(),
                    ));
                } else if !spec.fs_write.iter().any(|p| path_has_prefix(path, p)) {
                    errors.push(CapabilityError::new(
                        E1001,
                        format!(
                            "`write_file(\"{path}\", ...)` is not permitted by @[contained] \
                             (allowed prefixes: {})\n  \
                             help: Add `write(\"{pfx}\")` to the existing `fs: [...]` clause",
                            spec.fs_write.iter().map(|p| format!("\"{p}\"")).collect::<Vec<_>>().join(", ")
                        ),
                        Span::dummy(),
                    ));
                }
            }
        }

        IoKind::Net => {
            if let Some(host) = literal_arg {
                // 1. never: net check.
                for clause in &spec.never {
                    if let NeverClause::Net(glob) = clause {
                        if host_matches_glob(host, glob) {
                            errors.push(CapabilityError::new(
                                E1004,
                                format!(
                                    "`{name}(\"{host}\", ...)` is forbidden by `never: [net(\"{glob}\")]`\n  \
                                     help: remove the `never: [net(\"{glob}\")]` clause, or remove the network call — a `never` rule is a hard deny that no allowlist can override"
                                ),
                                Span::dummy(),
                            ));
                            return;
                        }
                    }
                }
                // 2. Allowlist check.
                if spec.net_allow.is_empty() {
                    errors.push(CapabilityError::new(
                        E1001,
                        format!(
                            "`{name}(\"{host}\", ...)` is not permitted: no `net: [...]` in @[contained]\n  \
                             help: Add `net: [\"{host}\"]` to the @[contained(...)] attribute to allow this call (or `net: [\"*.example.com\"]` for a host glob)"
                        ),
                        Span::dummy(),
                    ));
                } else if !spec.net_allow.iter().any(|g| host_matches_glob(host, g)) {
                    errors.push(CapabilityError::new(
                        E1001,
                        format!(
                            "`{name}(\"{host}\", ...)` is not permitted by @[contained] \
                             (allowed: {})\n  \
                             help: Add `\"{host}\"` to the existing `net: [...]` clause",
                            spec.net_allow.iter().map(|g| format!("\"{g}\"")).collect::<Vec<_>>().join(", ")
                        ),
                        Span::dummy(),
                    ));
                }
            }
        }

        IoKind::Exec => {
            // Check never: exec.
            if spec.never.iter().any(|c| matches!(c, NeverClause::Exec)) {
                errors.push(CapabilityError::new(
                    E1004,
                    format!(
                        "`{name}(...)` is forbidden by `never: [exec]`\n  \
                         help: remove the `never: [exec]` clause, or remove the exec call — a `never` rule is a hard deny that no allowlist can override"
                    ),
                    Span::dummy(),
                ));
            } else if !spec.exec_allowed {
                errors.push(CapabilityError::new(
                    E1001,
                    format!(
                        "`{name}(...)` is not permitted: `exec: none` or exec not specified in @[contained]\n  \
                         help: Add `exec: any` to the @[contained(...)] attribute to allow process spawning"
                    ),
                    Span::dummy(),
                ));
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ContainedSpec;
    use crate::span::Span;

    fn make_spec(fs_read: Vec<&str>, fs_write: Vec<&str>, never: Vec<NeverClause>) -> ContainedSpec {
        ContainedSpec {
            fs_read: fs_read.into_iter().map(String::from).collect(),
            fs_write: fs_write.into_iter().map(String::from).collect(),
            net_allow: Vec::new(),
            exec_allowed: false,
            never,
            span: Span::dummy(),
        }
    }

    #[test]
    fn allowed_read_produces_no_error() {
        let spec = make_spec(vec!["./data/"], vec![], vec![]);
        let args = vec![Expr::Literal(crate::ast::Literal::Str("./data/x.txt".into()))];
        let mut errors = Vec::new();
        check_call("read_file", &args, &spec, &mut errors);
        assert!(errors.is_empty(), "Expected no errors, got: {:?}", errors);
    }

    #[test]
    fn disallowed_write_produces_e1001() {
        let spec = make_spec(vec!["./data/"], vec![], vec![]);
        let args = vec![
            Expr::Literal(crate::ast::Literal::Str("/etc/passwd".into())),
            Expr::Literal(crate::ast::Literal::Str("x".into())),
        ];
        let mut errors = Vec::new();
        check_call("write_file", &args, &spec, &mut errors);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, E1001);
        // Bug #8: the message suggests the exact least-privilege clause.
        assert!(errors[0].message.contains("help:"), "no help line: {}", errors[0].message);
        assert!(errors[0].message.contains("write(\"/etc/\")"), "wrong suggestion: {}", errors[0].message);
    }

    #[test]
    fn never_read_produces_e1004() {
        let spec = make_spec(
            vec!["./data/", "/etc/"],  // /etc/ is in allowlist but also in never
            vec![],
            vec![NeverClause::Read("/etc/".into())],
        );
        let args = vec![Expr::Literal(crate::ast::Literal::Str("/etc/shadow".into()))];
        let mut errors = Vec::new();
        check_call("read_file", &args, &spec, &mut errors);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, E1004);
        // Bug #8: never-clause errors explain that no allowlist overrides them.
        assert!(errors[0].message.contains("help:"), "no help line: {}", errors[0].message);
        assert!(errors[0].message.contains("remove"), "should suggest removal: {}", errors[0].message);
    }

    #[test]
    fn dir_prefix_extracts_directory() {
        assert_eq!(dir_prefix("./data/x.txt"), "./data/");
        assert_eq!(dir_prefix("/etc/passwd"), "/etc/");
        assert_eq!(dir_prefix("bare.txt"), "bare.txt"); // no slash → whole path
        assert_eq!(dir_prefix("/"), "/");
        assert_eq!(dir_prefix("a/b/c/d"), "a/b/c/");
    }

    #[test]
    fn non_literal_arg_is_skipped() {
        let spec = make_spec(vec!["./data/"], vec![], vec![]);
        // Dynamic path — no literal string. INTENTIONAL v1 limitation: a computed
        // I/O target is not statically verifiable, so the capability check is
        // skipped (rather than denying all dynamic targets). This is a documented
        // residual sandbox gap closed only by the Phase-9 runtime `Sandbox<P>`;
        // LITERAL targets are precisely checked (incl. `..` traversal). See the
        // note on `literal_arg` in check_call.
        let args = vec![Expr::Ident("path".into())];
        let mut errors = Vec::new();
        check_call("read_file", &args, &spec, &mut errors);
        assert!(errors.is_empty(), "Non-literal path is not statically checked (v1 limitation)");
    }

    // ── R6 §4.4 import-edge capability check (E1203) — paired allow+deny (I-11) ──

    fn parse(src: &str) -> Program {
        crate::parse_source(src).expect("parse test program")
    }

    #[test]
    fn import_widening_capabilities_is_rejected() {
        // DENY: the importer is @[contained(fs: read only)] — it grants fs:read
        // but NOT net. An imported module that makes a network call widens the
        // boundary → E1203.
        let importer = parse(
            "@[contained(fs: [read(\"./data/\")], exec: none)]\n\
             fn main() -> i64 { 0 }",
        );
        let imported = parse(
            "fn fetch() -> str { match http_get(\"api.evil.com\") { Ok(s) => s  Err(_) => \"\" } }",
        );
        let errs = check_import_capabilities(&importer, "evil::net", &imported);
        assert_eq!(errs.len(), 1, "exactly one widening (net): {errs:?}");
        assert_eq!(errs[0].code, E1203);
        assert!(errs[0].message.contains("net"), "names the widened cap: {}", errs[0].message);
        assert!(errs[0].message.contains("evil::net"), "names the import: {}", errs[0].message);
    }

    #[test]
    fn import_within_grant_is_allowed() {
        // ALLOW: the importer grants fs:read; the imported module only reads a
        // file — within the grant, no E1203.
        let importer = parse(
            "@[contained(fs: [read(\"./data/\")], exec: none)]\n\
             fn main() -> i64 { 0 }",
        );
        let imported = parse(
            "fn load() -> str { match read_file(\"./data/x\") { Ok(s) => s  Err(_) => \"\" } }",
        );
        let errs = check_import_capabilities(&importer, "lib::loader", &imported);
        assert!(errs.is_empty(), "read within grant must be allowed: {errs:?}");
    }

    #[test]
    fn uncontained_importer_has_no_ceiling() {
        // BACK-COMPAT: an importer with NO @[contained] declares no boundary, so
        // importing a capability-exercising module is not a *widening* — E1203 is
        // opt-in. (This is why existing module-importing examples are unaffected.)
        let importer = parse("fn main() -> i64 { 0 }");
        let imported = parse(
            "fn fetch() -> str { match http_get(\"api.x\") { Ok(s) => s  Err(_) => \"\" } }",
        );
        let errs = check_import_capabilities(&importer, "any::net", &imported);
        assert!(errs.is_empty(), "uncontained importer has no ceiling to widen: {errs:?}");
    }

    #[test]
    fn multiple_widenings_each_reported_deterministically() {
        // The importer grants nothing (empty @[contained]); an import that both
        // reads files and hits the network widens on two axes → two E1203s, in a
        // stable (sorted) order.
        let importer = parse(
            "@[contained(exec: none)]\n\
             fn main() -> i64 { 0 }",
        );
        let imported = parse(
            "fn act() -> i64 {\n\
               let _ = read_file(\"/etc/passwd\")\n\
               let _ = http_get(\"api.x\")\n\
               0\n\
             }",
        );
        let errs = check_import_capabilities(&importer, "m", &imported);
        assert_eq!(errs.len(), 2, "two widenings: {errs:?}");
        assert!(errs.iter().all(|e| e.code == E1203));
        // BTreeSet difference is sorted: fs:read before net.
        assert!(errs[0].message.contains("fs:read"), "first: {}", errs[0].message);
        assert!(errs[1].message.contains("net"), "second: {}", errs[1].message);
    }
}
