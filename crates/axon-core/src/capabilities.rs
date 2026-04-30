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

pub const E1001: &'static str = "E1001";
pub const E1002: &'static str = "E1002";
pub const E1003: &'static str = "E1003";
pub const E1004: &'static str = "E1004";

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
    Exec,
}

/// Match a function name to an I/O kind.
fn classify_call(name: &str) -> Option<IoKind> {
    match name {
        "read_file"                             => Some(IoKind::FsRead),
        "write_file"                            => Some(IoKind::FsWrite),
        // Future net calls (http_get, ai_complete, etc.) — treat as net
        "http_get" | "http_post" | "ai_complete" => Some(IoKind::Net),
        _ => None,
    }
}

// ── Path / host matching ──────────────────────────────────────────────────────

/// Returns true if `path` has `prefix` as a path prefix.
/// e.g. `path_has_prefix("./data/x.txt", "./data/")` → true
fn path_has_prefix(path: &str, prefix: &str) -> bool {
    path.starts_with(prefix)
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

// ── Core check ───────────────────────────────────────────────────────────────

/// Run the capability check on all functions in `program` and return diagnostics.
pub fn check_capabilities(program: &Program) -> Vec<CapabilityError> {
    let mut errors = Vec::new();
    for item in &program.items {
        if let Item::FnDef(fndef) = item {
            check_fn(fndef, &mut errors);
        }
    }
    errors
}

fn check_fn(fndef: &FnDef, errors: &mut Vec<CapabilityError>) {
    let spec = match &fndef.contained {
        Some(s) => s,
        None => return, // no @[contained] — no restrictions
    };
    // Walk the function body.
    check_expr(&fndef.body, spec, errors);
}

fn check_stmts(stmts: &[Stmt], spec: &ContainedSpec, errors: &mut Vec<CapabilityError>) {
    for stmt in stmts {
        check_expr(&stmt.expr, spec, errors);
    }
}

fn check_expr(expr: &Expr, spec: &ContainedSpec, errors: &mut Vec<CapabilityError>) {
    match expr {
        Expr::Call { callee, args } => {
            if let Expr::Ident(name) = callee.as_ref() {
                check_call(name, args, spec, errors);
            }
            // Recurse into callee and args.
            check_expr(callee, spec, errors);
            for arg in args {
                check_expr(arg, spec, errors);
            }
        }

        // Recurse into all compound expressions.
        Expr::Block(stmts) => check_stmts(stmts, spec, errors),
        Expr::Let { value, .. }
        | Expr::Own { value, .. }
        | Expr::RefBind { value, .. } => check_expr(value, spec, errors),
        Expr::BinOp { left, right, .. } => {
            check_expr(left, spec, errors);
            check_expr(right, spec, errors);
        }
        Expr::UnaryOp { operand, .. } => check_expr(operand, spec, errors),
        Expr::Question(inner) => check_expr(inner, spec, errors),
        Expr::MethodCall { receiver, args, .. } => {
            check_expr(receiver, spec, errors);
            for arg in args {
                check_expr(arg, spec, errors);
            }
        }
        Expr::If { cond, then, else_ } => {
            check_expr(cond, spec, errors);
            check_expr(then, spec, errors);
            if let Some(e) = else_ {
                check_expr(e, spec, errors);
            }
        }
        Expr::Match { subject, arms } => {
            check_expr(subject, spec, errors);
            for arm in arms {
                if let Some(g) = &arm.guard { check_expr(g, spec, errors); }
                check_expr(&arm.body, spec, errors);
            }
        }
        Expr::While { cond, body } => {
            check_expr(cond, spec, errors);
            check_stmts(body, spec, errors);
        }
        Expr::WhileLet { expr, body, .. } => {
            check_expr(expr, spec, errors);
            check_stmts(body, spec, errors);
        }
        Expr::For { start, end, body, .. } => {
            check_expr(start, spec, errors);
            check_expr(end, spec, errors);
            check_stmts(body, spec, errors);
        }
        Expr::Assign { value, .. } => check_expr(value, spec, errors),
        Expr::Return(inner) => {
            if let Some(e) = inner { check_expr(e, spec, errors); }
        }
        Expr::FieldAccess { receiver, .. } => check_expr(receiver, spec, errors),
        Expr::Index { receiver, index } => {
            check_expr(receiver, spec, errors);
            check_expr(index, spec, errors);
        }
        Expr::Ok(inner) | Expr::Err(inner) | Expr::Some(inner) => {
            check_expr(inner, spec, errors);
        }
        Expr::Array(elems) => {
            for e in elems { check_expr(e, spec, errors); }
        }
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields { check_expr(v, spec, errors); }
        }
        Expr::FmtStr { parts } => {
            for part in parts {
                if let crate::ast::FmtPart::Expr(e) = part {
                    check_expr(e, spec, errors);
                }
            }
        }
        Expr::Lambda { body, .. } => check_expr(body, spec, errors),
        Expr::Spawn(body) => check_expr(body, spec, errors),
        Expr::Select(arms) => {
            for arm in arms {
                check_expr(&arm.recv, spec, errors);
                check_expr(&arm.body, spec, errors);
            }
        }
        Expr::Comptime(inner) => check_expr(inner, spec, errors),
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

    // Extract a literal string argument (first arg for read_file/write_file/http calls).
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
                // 1. Check never: rules first (hard violation).
                for clause in &spec.never {
                    if let NeverClause::Read(prefix) = clause {
                        if path_has_prefix(path, prefix) {
                            errors.push(CapabilityError::new(
                                E1004,
                                format!(
                                    "`read_file(\"{path}\")` is forbidden by `never: [read(\"{prefix}\")]`"
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
                            "`read_file(\"{path}\")` is not permitted: no `fs: [read(...)]` in @[contained]"
                        ),
                        Span::dummy(),
                    ));
                } else if !spec.fs_read.iter().any(|p| path_has_prefix(path, p)) {
                    errors.push(CapabilityError::new(
                        E1001,
                        format!(
                            "`read_file(\"{path}\")` is not permitted by @[contained] \
                             (allowed prefixes: {})",
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
                // 1. never: write check.
                for clause in &spec.never {
                    if let NeverClause::Write(prefix) = clause {
                        if path_has_prefix(path, prefix) {
                            errors.push(CapabilityError::new(
                                E1004,
                                format!(
                                    "`write_file(\"{path}\", ...)` is forbidden by `never: [write(\"{prefix}\")]`"
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
                            "`write_file(\"{path}\", ...)` is not permitted: no `fs: [write(...)]` in @[contained]"
                        ),
                        Span::dummy(),
                    ));
                } else if !spec.fs_write.iter().any(|p| path_has_prefix(path, p)) {
                    errors.push(CapabilityError::new(
                        E1001,
                        format!(
                            "`write_file(\"{path}\", ...)` is not permitted by @[contained] \
                             (allowed prefixes: {})",
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
                                    "`{name}(\"{host}\", ...)` is forbidden by `never: [net(\"{glob}\")]`"
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
                            "`{name}(\"{host}\", ...)` is not permitted: no `net: [...]` in @[contained]"
                        ),
                        Span::dummy(),
                    ));
                } else if !spec.net_allow.iter().any(|g| host_matches_glob(host, g)) {
                    errors.push(CapabilityError::new(
                        E1001,
                        format!(
                            "`{name}(\"{host}\", ...)` is not permitted by @[contained] \
                             (allowed: {})",
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
                    format!("`{name}(...)` is forbidden by `never: [exec]`"),
                    Span::dummy(),
                ));
            } else if !spec.exec_allowed {
                errors.push(CapabilityError::new(
                    E1001,
                    format!("`{name}(...)` is not permitted: `exec: none` or exec not specified in @[contained]"),
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
    }

    #[test]
    fn non_literal_arg_is_skipped() {
        let spec = make_spec(vec!["./data/"], vec![], vec![]);
        // Dynamic path — no literal string
        let args = vec![Expr::Ident("path".into())];
        let mut errors = Vec::new();
        check_call("read_file", &args, &spec, &mut errors);
        assert!(errors.is_empty(), "Non-literal path should not produce static error");
    }
}
