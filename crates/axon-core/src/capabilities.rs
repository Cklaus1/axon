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
        Self {
            code,
            message: message.into(),
            span,
        }
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
    // Reading the process environment (`env_var`). Unlike fs/net/exec there is NO
    // allowlist clause that can grant it, so inside `@[contained]` it is denied
    // unconditionally — env vars are an ambient, secret-bearing channel (API
    // keys, tokens) and a sandbox that let ungranted env reads through would not
    // actually contain exfiltration. The fix is to read the env OUTSIDE the
    // contained boundary and pass the value in explicitly.
    Env,
}

/// Match a function name to an I/O kind.
fn classify_call(name: &str) -> Option<IoKind> {
    match name {
        "read_file" => Some(IoKind::FsRead),
        "write_file" => Some(IoKind::FsWrite),
        "exec" => Some(IoKind::Exec),
        // Reading the process environment is an ungrantable ambient channel; a
        // @[contained] fn must not read host secrets it wasn't given.
        "env_var" => Some(IoKind::Env),
        // Future net calls (http_get, ai_complete, etc.) — treat as net
        "http_get"
        | "http_post"
        | "http_sse"
        | "http_sse_post"
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

/// The Anthropic API endpoint every AI builtin implicitly contacts.
/// `@[contained(net: ["api.anthropic.com"])]` grants exactly this.
const ANTHROPIC_HOST: &str = "api.anthropic.com";

/// For an AI builtin (`ai_complete`, `ai_extract_*`), the network host is FIXED
/// (the Anthropic endpoint) and the first call argument is the PROMPT, not a
/// host. Returns the implicit host for those builtins, or `None` for a net
/// builtin whose first arg genuinely IS the host (`http_get`/`http_post`).
fn ai_builtin_host(name: &str) -> Option<&'static str> {
    if name == "ai_complete"
        || name == "ai_extract_uncertain_i64"
        || name == "ai_extract_uncertain_f64"
        || name.starts_with("ai_extract::<")
    {
        Some(ANTHROPIC_HOST)
    } else {
        None
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
    // method-name → every impl method with that name. The MethodCall walk uses
    // this to follow `x.go()` into the method body transitively. Dispatch is
    // OVER-APPROXIMATED by name: the static checker has no receiver type, so a
    // contained fn calling `x.go()` is checked against EVERY `go` impl. This
    // cannot be laundered (no false negative — the soundness property a security
    // boundary needs); at worst it over-reports when two types share a method
    // name and the caller uses the pure one (documented limitation).
    let mut method_map: std::collections::HashMap<&str, Vec<&FnDef>> =
        std::collections::HashMap::new();
    for item in &program.items {
        if let Item::ImplBlock(b) = item {
            for m in &b.methods {
                method_map.entry(m.name.as_str()).or_default().push(m);
            }
        }
    }
    // R13 native FFI: the import/call capability gate (fail-closed, I-11). Any
    // fn whose body calls a native module `M::*` must declare a matching
    // `@[contained(M: …)]` grant; an ungranted call is E1004. This is checked
    // per-fn on the DIRECT call sites: a helper that hides a `gfx::*` call is
    // itself ungranted → E1004 on the helper (no laundering past the boundary).
    for item in &program.items {
        match item {
            Item::FnDef(f) => check_native_grants(f, &mut errors),
            Item::ImplBlock(b) => {
                for m in &b.methods {
                    check_native_grants(m, &mut errors);
                }
            }
            _ => {}
        }
    }
    for item in &program.items {
        match item {
            Item::FnDef(fndef) => check_fn(fndef, &fn_map, &method_map, &mut errors),
            // An impl method with its OWN @[contained] must be enforced too —
            // otherwise `impl T for X { @[contained(net:[])] fn m(self){ http_get(..) } }`
            // would silently escape its declared sandbox (the method's body was
            // never checked against its spec). Methods without @[contained] are
            // no-ops in check_fn, matching free-fn behavior.
            Item::ImplBlock(b) => {
                for m in &b.methods {
                    check_fn(m, &fn_map, &method_map, &mut errors);
                }
            }
            _ => {}
        }
    }
    errors
}

fn check_fn<'a>(
    fndef: &'a FnDef,
    fn_map: &'a std::collections::HashMap<&'a str, &'a FnDef>,
    method_map: &'a std::collections::HashMap<&'a str, Vec<&'a FnDef>>,
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
    let mut ctx = CapCtx {
        spec,
        fn_map,
        method_map,
        visited,
        visited_methods: std::collections::HashSet::new(),
        errors,
    };
    check_expr(&fndef.body, &mut ctx);
}

/// R13 native FFI: enforce that every native `M::*` call in `fndef`'s body is
/// covered by a `@[contained(M: …)]` grant on this fn (E1004, fail-closed).
/// Reuses the single exhaustive, laundering-closed `collect_caps_expr` walker
/// (which records native calls as `native:M`), so there is no second walk to
/// drift out of sync.
fn check_native_grants(fndef: &FnDef, errors: &mut Vec<CapabilityError>) {
    let mut caps: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    collect_caps_expr(&fndef.body, &mut caps);
    let called: Vec<&str> = caps
        .iter()
        .filter_map(|c| c.strip_prefix("native:"))
        .collect();
    if called.is_empty() {
        return;
    }
    let granted: std::collections::BTreeSet<&str> = fndef
        .contained
        .as_ref()
        .map(|s| s.native_grants.iter().map(String::as_str).collect())
        .unwrap_or_default();
    for module in called {
        if !granted.contains(module) {
            errors.push(CapabilityError::new(
                E1004,
                format!(
                    "`native::{module}` requires a `{module}` capability grant — add \
                     `@[contained({module}: any)]` to `{}` to permit it",
                    fndef.name
                ),
                fndef.span,
            ));
        }
    }
}

/// Threaded state for the transitive `@[contained]` walk: the spec being
/// enforced, the program's fn map (to follow helper calls) and method map (to
/// follow `x.go()` into impl bodies), the visited sets (to stop recursion), and
/// the error sink.
struct CapCtx<'a, 'e> {
    spec: &'a ContainedSpec,
    fn_map: &'a std::collections::HashMap<&'a str, &'a FnDef>,
    method_map: &'a std::collections::HashMap<&'a str, Vec<&'a FnDef>>,
    visited: std::collections::HashSet<&'a str>,
    /// Method names already descended into on this path (cycle guard, separate
    /// namespace from free-fn `visited` since a method and a fn can share a name).
    visited_methods: std::collections::HashSet<&'a str>,
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
        match item {
            Item::FnDef(f) => collect_caps_expr(&f.body, &mut caps),
            // Impl-method bodies are full FnDefs and CAN exercise capabilities —
            // an exfil call inside `impl Trait for T { fn m(self) { http_get(..) } }`
            // was previously invisible to G2's capability-diff (I-12) and the
            // import-edge demand set (I-11, E1203), a real laundering vector.
            Item::ImplBlock(b) => {
                for m in &b.methods {
                    collect_caps_expr(&m.body, &mut caps);
                }
            }
            // Module-level `let NAME = comptime { … }` initializers run code too.
            Item::LetDef { value, .. } => collect_caps_expr(value, &mut caps),
            _ => {}
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

/// A capability-bearing builtin (`read_file`, `ai_complete`, `exec`, …) used as a
/// VALUE rather than called directly — e.g. `let f = read_file; f(p)`. There is no
/// call site to path/host-check, so a bare reference is the capability itself; it is
/// permitted only if the spec grants that whole category, otherwise it is an E1001.
///
/// Closes the builtin-aliasing laundering route (THREAT_MODEL.md §8): without this,
/// aliasing a forbidden builtin to a binding passed `axon check` (caught only by the
/// interpreter at runtime, and a live hole the day builtins become first-class).
///
/// Name-based, like the rest of this checker: a local shadowing a builtin name is
/// over-approximated (may over-report) — consistent with the method-dispatch and
/// helper-follow checks, and sound (no false negative).
fn check_builtin_value_ref(name: &str, spec: &ContainedSpec, errors: &mut Vec<CapabilityError>) {
    let cap = match capability_of_builtin(name) {
        Some(c) => c,
        None => return, // pure builtin, or not a builtin — fine as a value
    };
    let forbidden = match cap {
        "fs:read" => spec.fs_read.is_empty(),
        "fs:write" => spec.fs_write.is_empty(),
        "net" => spec.net_allow.is_empty(),
        "exec" => !spec.exec_allowed,
        _ => false,
    };
    if forbidden {
        errors.push(CapabilityError::new(
            E1001,
            format!(
                "builtin `{name}` (capability `{cap}`) is not permitted as a value: \
                 no matching grant in @[contained]\n  \
                 help: a capability-bearing builtin cannot be aliased or passed as a \
                 value inside a @[contained] fn unless the `{cap}` capability is granted \
                 (it would let the call escape the path/host check)"
            ),
            Span::dummy(),
        ));
    }
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
    // Closure so a contained spec on either a free fn OR an impl method raises
    // the ceiling consistently (an impl method may legitimately declare its own
    // grant; counting it keeps the import-edge demand/grant sides symmetric).
    let mut absorb = |spec: &ContainedSpec| {
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
    };
    for item in &importer.items {
        match item {
            Item::FnDef(f) => {
                if let Some(spec) = &f.contained {
                    absorb(spec);
                }
            }
            Item::ImplBlock(b) => {
                for m in &b.methods {
                    if let Some(spec) = &m.contained {
                        absorb(spec);
                    }
                }
            }
            _ => {}
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
        IoKind::Env => "env",
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
                // R13: a native `M::*` call is a capability — record it as
                // `native:M` so the import-edge (E1203) and grant (E1004) checks
                // both see it through this single exhaustive, laundering-closed
                // walker.
                if let Some((m, _f)) = crate::native::resolve_call(n) {
                    caps.insert(format!("native:{}", m.name));
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
        Expr::For {
            start, end, body, ..
        } => {
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
        // These forms were missing — a capability call inside any of them was
        // INVISIBLE to the import-edge E1203 check, so an imported module could
        // launder a capability past the importer's @[contained] ceiling through a
        // `with` block / spawn / select / comptime. Kept in lockstep with the
        // self-check walker `check_expr` (which already handles all of these).
        Expr::WithHandler { handler, body } => {
            if let crate::ast::HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                for arm in arms {
                    collect_caps_expr(&arm.body, caps);
                }
                if let Some(ra) = return_arm {
                    collect_caps_expr(&ra.body, caps);
                }
            }
            collect_caps_expr(body, caps);
        }
        Expr::Spawn(inner) | Expr::Comptime(inner) => collect_caps_expr(inner, caps),
        Expr::Select(arms) => {
            for arm in arms {
                collect_caps_expr(&arm.recv, caps);
                collect_caps_expr(&arm.body, caps);
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
            // Recurse into args. Recurse into the callee ONLY when it is not a
            // plain ident: a plain-ident callee is already fully handled above
            // (builtin via check_call, user fn via the helper-follow), and the
            // Ident arm below now flags builtin VALUE references — so recursing a
            // plain-ident callee here would double-report it as an alias.
            if !matches!(callee.as_ref(), Expr::Ident(_)) {
                check_expr(callee, ctx);
            }
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
        Expr::Let { value, .. } | Expr::Own { value, .. } | Expr::RefBind { value, .. } => {
            check_expr(value, ctx)
        }
        Expr::BinOp { left, right, .. } => {
            check_expr(left, ctx);
            check_expr(right, ctx);
        }
        Expr::UnaryOp { operand, .. } => check_expr(operand, ctx),
        Expr::Question(inner) => check_expr(inner, ctx),
        Expr::MethodCall {
            receiver,
            method,
            args,
        } => {
            check_expr(receiver, ctx);
            for arg in args {
                check_expr(arg, ctx);
            }
            // Follow the dispatch into the impl method body so a contained fn
            // cannot launder a forbidden call through `x.go()`. Dispatch is
            // over-approximated by NAME (no receiver type statically): every impl
            // method named `method` is checked under the caller's spec. Sound
            // (no false negative); may over-report when a name is shared. A
            // method with its OWN @[contained] is also checked under its own spec
            // elsewhere; descending here additionally constrains it by the
            // (possibly stricter) CALLER spec — defense in depth, same as helpers.
            if let Some(impls) = ctx.method_map.get(method.as_str()) {
                if ctx.visited_methods.insert(method.as_str()) {
                    for m in impls {
                        check_expr(&m.body, ctx);
                    }
                    ctx.visited_methods.remove(method.as_str());
                }
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
                if let Some(g) = &arm.guard {
                    check_expr(g, ctx);
                }
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
        Expr::For {
            start, end, body, ..
        } => {
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
            if let Some(e) = inner {
                check_expr(e, ctx);
            }
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
            for e in elems {
                check_expr(e, ctx);
            }
        }
        Expr::StructLit { fields, .. } => {
            for (_, v) in fields {
                check_expr(v, ctx);
            }
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
        // A capability-bearing builtin referenced as a VALUE (not called) —
        // `let f = read_file; f(p)`. Closes the builtin-aliasing route
        // (THREAT_MODEL.md §8); permitted only if the spec grants the category.
        Expr::Ident(name) => check_builtin_value_ref(name, ctx.spec, ctx.errors),
        // Leaf nodes — no recursion needed.
        Expr::Literal(_)
        | Expr::None
        | Expr::Break
        | Expr::Continue
        | Expr::InlineAsm { .. } => {}
    }
}

/// Enforce a net `host` against the spec: a `never: [net(...)]` hard-deny
/// (E1004), then the `net: [...]` allowlist (E1001 when empty or unmatched).
/// Factored out so BOTH the builtin net calls (`http_get`/`ai_complete`) AND
/// the R22 native-module connect calls (`modbus_connect`/`fhir_connect`) pin to
/// the ACTUAL connect host through one mechanism (the `ai-complete host check`
/// lesson: the net cap checks the real host, not a prompt).
fn check_net_host(
    host: &str,
    call_display: &dyn Fn(&str) -> String,
    spec: &ContainedSpec,
    errors: &mut Vec<CapabilityError>,
) {
    // 1. never: net check.
    for clause in &spec.never {
        if let NeverClause::Net(glob) = clause {
            if host_matches_glob(host, glob) {
                errors.push(CapabilityError::new(
                    E1004,
                    format!(
                        "`{}` is forbidden by `never: [net(\"{glob}\")]`\n  \
                         help: remove the `never: [net(\"{glob}\")]` clause, or remove the network call — a `never` rule is a hard deny that no allowlist can override",
                        call_display(host)
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
                "`{}` is not permitted: no `net: [...]` in @[contained]\n  \
                 help: Add `net: [\"{host}\"]` to the @[contained(...)] attribute to allow this call (or `net: [\"*.example.com\"]` for a host glob)",
                call_display(host)
            ),
            Span::dummy(),
        ));
    } else if !spec.net_allow.iter().any(|g| host_matches_glob(host, g)) {
        errors.push(CapabilityError::new(
            E1001,
            format!(
                "`{}` is not permitted by @[contained] (allowed: {})\n  \
                 help: Add `\"{host}\"` to the existing `net: [...]` clause",
                call_display(host),
                spec.net_allow
                    .iter()
                    .map(|g| format!("\"{g}\""))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Span::dummy(),
        ));
    }
}

/// R22: the host literal of a native connect call. A native module that
/// declares the `Net` effect and whose fn takes a leading `str` host param
/// (`modbus_connect(host, port)`, `fhir_connect(base_url)`) must pin that host
/// against the `net` allowlist — reusing the existing host-pinning mechanism.
/// Returns the host string for such a call (the first str arg, with any
/// `scheme://` and `:port/path` stripped for a base-URL form), else `None`.
fn native_net_host(name: &str, args: &[Expr]) -> Option<String> {
    let (module, _nf) = crate::native::resolve_call(name)?;
    if !module.effects.contains(&"Net") {
        return None;
    }
    // The host is the first `str` literal arg of a *connect* fn.
    if !name.ends_with("_connect") {
        return None;
    }
    let lit = match args.first()? {
        Expr::Literal(crate::ast::Literal::Str(s)) => s.as_str(),
        // A dynamically-built host can't be statically verified → fail closed.
        _ => return Some(String::from("\u{0}dynamic")),
    };
    Some(host_of(lit))
}

/// Extract the bare host from a literal that may be a plain host or a base URL
/// (`http://host:port/path` → `host`).
fn host_of(s: &str) -> String {
    let after_scheme = s.split_once("://").map(|(_, rest)| rest).unwrap_or(s);
    let host_port = after_scheme.split('/').next().unwrap_or(after_scheme);
    host_port
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(host_port)
        .to_string()
}

/// Validate a single I/O call against the spec.
fn check_call(name: &str, args: &[Expr], spec: &ContainedSpec, errors: &mut Vec<CapabilityError>) {
    // R22: a native net-connect call pins its host against the `net` allowlist
    // (the same mechanism as builtin net calls). This runs IN ADDITION to the
    // `native:M` grant gate (E1004 in `check_native_grants`).
    if let Some(host) = native_net_host(name, args) {
        let display_name = name.to_string();
        if host == "\u{0}dynamic" {
            let help = if spec.net_allow.is_empty() {
                "no network capability is granted; add a `net: [\"...\"]` clause and use a LITERAL host"
            } else {
                "a dynamically-built host cannot be statically verified against the sandbox; use a literal host"
            };
            errors.push(CapabilityError::new(
                E1001,
                format!("`{display_name}(<dynamic host>, ...)` is not permitted by @[contained]\n  help: {help}"),
                Span::dummy(),
            ));
        } else {
            let display = move |h: &str| format!("{display_name}(\"{h}\", ...)");
            check_net_host(&host, &display, spec, errors);
        }
    }
    let kind = match classify_call(name) {
        Some(k) => k,
        None => return, // not an I/O builtin
    };

    // Extract a literal string argument (first arg for read_file/write_file/http
    // calls).
    //
    // STATIC-ANALYSIS BOUNDARY (refined): the *precise* target check (which
    // path/host, `..` traversal, allowlist match) only applies to a LITERAL
    // target — a computed or string-interpolated target is `None` here and the
    // checker can't know its value. BUT an EMPTY allowlist grants ZERO
    // capability of that kind, so the target is irrelevant: the per-kind
    // branches below DENY a dynamic target when the relevant allowlist is empty
    // (`fs_read`/`fs_write`/`net_allow`). This closes the laundering hole where
    // `read_file("/etc/{p}")` / `ai_complete("leak {x}")` (interpolated args,
    // not `Literal::Str`) slipped past `fs: []` / `net: []` — the capability
    // boundary now fails CLOSED. A dynamic target against a NON-empty allowlist
    // stays runtime-deferred (Phase-9 `Sandbox<P>`): the fn already holds the
    // capability; only the specific target awaits runtime enforcement.
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
                            spec.fs_read
                                .iter()
                                .map(|p| format!("\"{p}\""))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        Span::dummy(),
                    ));
                }
            } else {
                // Non-literal (dynamic / string-interpolated) path. We cannot
                // match the target against the allowlist, and `@[contained]` has
                // no runtime target enforcement yet, so the capability boundary
                // fails CLOSED (sound by refusal) — NOT open. An empty allowlist
                // grants zero read capability; a NON-empty one can't constrain a
                // dynamic path (it could escape via `..` or be built to any
                // value), so an unverifiable target is refused either way. This
                // closes the laundering hole where `read_file(p)` /
                // `read_file("/etc/{p}")` slipped past a `fs: [read("./ok/")]`
                // allowlist (the dynamic arg is not a `Literal::Str`).
                let help = if spec.fs_read.is_empty() {
                    "no read capability is granted; add an `fs: [read(\"...\")]` clause and use a LITERAL path"
                } else {
                    "a dynamically-built path cannot be statically verified against the sandbox (it could escape the allowlist); use a literal path"
                };
                errors.push(CapabilityError::new(
                    E1001,
                    format!("`read_file(<dynamic path>)` is not permitted by @[contained]\n  help: {help}"),
                    Span::dummy(),
                ));
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
                            spec.fs_write
                                .iter()
                                .map(|p| format!("\"{p}\""))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        Span::dummy(),
                    ));
                }
            } else {
                // Dynamic/interpolated path → fail CLOSED (see FsRead above): a
                // non-literal path can't be verified against the allowlist, and
                // there is no runtime target enforcement, so a write to an
                // unprovable path is refused whether or not the allowlist is
                // empty. Closes the `write_file(p, ...)` / `write_file("/etc/{p}",
                // ...)` launder past a non-empty `fs: [write("./out/")]`.
                let help = if spec.fs_write.is_empty() {
                    "no write capability is granted; add an `fs: [write(\"...\")]` clause and use a LITERAL path"
                } else {
                    "a dynamically-built path cannot be statically verified against the sandbox (it could escape the allowlist); use a literal path"
                };
                errors.push(CapabilityError::new(
                    E1001,
                    format!("`write_file(<dynamic path>, ...)` is not permitted by @[contained]\n  help: {help}"),
                    Span::dummy(),
                ));
            }
        }

        IoKind::Net => {
            // CRITICAL: the network HOST is not always the first argument. For
            // `http_get(url)`/`http_post(url, …)` the first arg IS the host/URL,
            // so the allowlist is checked against `literal_arg`. But for the AI
            // builtins (`ai_complete`, `ai_extract_*`) the first arg is the
            // PROMPT — the host is implicitly the Anthropic API endpoint. Checking
            // the prompt against a host allowlist is just wrong (it denied every
            // `ai_complete` under a host-pinned `net: ["api.anthropic.com"]`, and
            // would "allow" a prompt that happened to match a glob). So the
            // effective host for an AI builtin is the fixed endpoint, regardless
            // of the prompt's content or whether it's a literal.
            let ai_host = ai_builtin_host(name);
            // For `http_get`/`http_post`/`http_sse*` the first arg is a full URL, not
            // a bare host. Normalize it to its host (strip `scheme://`, `:port`, and
            // `/path`) so a real URL like `https://api.openai.com/v1/models` matches a
            // host allowlist of `api.openai.com` — the same stripping native
            // net-connect calls already use via `native_net_host`. Without this,
            // host-pinning was unusable with real URLs (every URL was refused because
            // the whole string never equals the bare host). AI builtins keep their
            // fixed implicit host.
            let normalized_url: Option<String> = if ai_host.is_none() {
                literal_arg.map(host_of)
            } else {
                None
            };
            let effective_host: Option<&str> = ai_host.or(normalized_url.as_deref());
            // How to render the call in diagnostics: an AI builtin's host is
            // implicit, so show `ai_complete(...) [host api.anthropic.com]`
            // rather than misleadingly printing the prompt as the first-arg host.
            let call_display = |host: &str| -> String {
                if ai_host.is_some() {
                    format!("{name}(...) [host {host}]")
                } else {
                    format!("{name}(\"{host}\", ...)")
                }
            };
            if let Some(host) = effective_host {
                check_net_host(host, &call_display, spec, errors);
            } else {
                // Dynamic host (a non-AI net call like `http_get(url)` with a
                // computed URL — AI builtins have a fixed `ai_host`, so they took
                // the branch above). Fail CLOSED: an unverifiable host can't be
                // matched against the allowlist and there's no runtime check, so
                // it's refused whether or not the allowlist is empty. Closes the
                // launder past a non-empty `net: ["ok.com"]` via a computed URL.
                let help = if spec.net_allow.is_empty() {
                    "no network capability is granted; add a `net: [\"...\"]` clause and use a LITERAL host"
                } else {
                    "a dynamically-built host cannot be statically verified against the sandbox; use a literal host"
                };
                errors.push(CapabilityError::new(
                    E1001,
                    format!("`{name}(<dynamic argument>)` is not permitted by @[contained]\n  help: {help}"),
                    Span::dummy(),
                ));
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

        IoKind::Env => {
            // No allowlist clause can grant environment access, so a @[contained]
            // fn may NOT read the process environment — env vars are an ambient,
            // secret-bearing channel (API keys, tokens), and permitting an
            // ungranted read would let sandboxed code exfiltrate host secrets
            // (defeating the whole point of containment). Always deny.
            errors.push(CapabilityError::new(
                E1001,
                format!(
                    "`{name}(...)` is not permitted inside @[contained]: reading the process \
                     environment is an ungovernable ambient channel (env vars often hold secrets) \
                     and there is no capability clause that can grant it\n  \
                     help: read the environment OUTSIDE the contained boundary and pass the value \
                     in as an argument, so the sandboxed code only sees what you explicitly hand it"
                ),
                Span::dummy(),
            ));
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ContainedSpec;
    use crate::span::Span;

    fn make_spec(
        fs_read: Vec<&str>,
        fs_write: Vec<&str>,
        never: Vec<NeverClause>,
    ) -> ContainedSpec {
        ContainedSpec {
            fs_read: fs_read.into_iter().map(String::from).collect(),
            fs_write: fs_write.into_iter().map(String::from).collect(),
            net_allow: Vec::new(),
            exec_allowed: false,
            never,
            native_grants: Vec::new(),
            span: Span::dummy(),
        }
    }

    /// Build a spec granting a net allowlist (make_spec hardcodes empty net).
    fn make_net_spec(net_allow: Vec<&str>) -> ContainedSpec {
        ContainedSpec {
            fs_read: Vec::new(),
            fs_write: Vec::new(),
            net_allow: net_allow.into_iter().map(String::from).collect(),
            exec_allowed: false,
            never: Vec::new(),
            native_grants: Vec::new(),
            span: Span::dummy(),
        }
    }

    #[test]
    fn ai_complete_checks_implicit_host_not_prompt() {
        // REGRESSION: ai_complete's first arg is the PROMPT, not a host. The net
        // check must validate the implicit Anthropic endpoint against the
        // allowlist — NOT the prompt string. Before the fix, ai_complete under
        // `net: ["api.anthropic.com"]` was wrongly DENIED (the prompt didn't match
        // the host glob), which broke every realistic LLM-agent use case.
        let spec = make_net_spec(vec!["api.anthropic.com"]);
        // A long prose prompt that would never match a host glob.
        let args = vec![Expr::Literal(crate::ast::Literal::Str(
            "Summarize these notes concisely for a tweet.".into(),
        ))];
        let mut errors = Vec::new();
        check_call("ai_complete", &args, &spec, &mut errors);
        assert!(errors.is_empty(),
            "ai_complete under the anthropic grant must be allowed regardless of prompt: {errors:?}");
    }

    #[test]
    fn ai_complete_denied_without_anthropic_grant() {
        // The deny side: ai_complete under a net allowlist that does NOT include
        // the Anthropic host is refused (E1001), and the message names the host,
        // not the prompt.
        let spec = make_net_spec(vec!["api.other.com"]);
        let args = vec![Expr::Literal(crate::ast::Literal::Str("any prompt".into()))];
        let mut errors = Vec::new();
        check_call("ai_complete", &args, &spec, &mut errors);
        assert_eq!(
            errors.len(),
            1,
            "ai_complete without anthropic grant must be denied: {errors:?}"
        );
        assert_eq!(errors[0].code, E1001);
        assert!(
            errors[0].message.contains("api.anthropic.com"),
            "message should name the implicit host: {}",
            errors[0].message
        );
        assert!(
            !errors[0].message.contains("any prompt"),
            "message must NOT print the prompt as the host: {}",
            errors[0].message
        );
    }

    #[test]
    fn http_get_still_checks_first_arg_as_host() {
        // The non-AI net builtins are unchanged: http_get's first arg IS the host.
        let spec = make_net_spec(vec!["api.allowed.com"]);
        let mut errors = Vec::new();
        check_call(
            "http_get",
            &[Expr::Literal(crate::ast::Literal::Str(
                "api.allowed.com".into(),
            ))],
            &spec,
            &mut errors,
        );
        assert!(errors.is_empty(), "http_get to an allowed host: {errors:?}");
        let mut errors = Vec::new();
        check_call(
            "http_get",
            &[Expr::Literal(crate::ast::Literal::Str(
                "api.evil.com".into(),
            ))],
            &spec,
            &mut errors,
        );
        assert_eq!(
            errors.len(),
            1,
            "http_get to a non-allowed host must be denied: {errors:?}"
        );
    }

    #[test]
    fn allowed_read_produces_no_error() {
        let spec = make_spec(vec!["./data/"], vec![], vec![]);
        let args = vec![Expr::Literal(crate::ast::Literal::Str(
            "./data/x.txt".into(),
        ))];
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
        assert!(
            errors[0].message.contains("help:"),
            "no help line: {}",
            errors[0].message
        );
        assert!(
            errors[0].message.contains("write(\"/etc/\")"),
            "wrong suggestion: {}",
            errors[0].message
        );
    }

    #[test]
    fn never_read_produces_e1004() {
        let spec = make_spec(
            vec!["./data/", "/etc/"], // /etc/ is in allowlist but also in never
            vec![],
            vec![NeverClause::Read("/etc/".into())],
        );
        let args = vec![Expr::Literal(crate::ast::Literal::Str(
            "/etc/shadow".into(),
        ))];
        let mut errors = Vec::new();
        check_call("read_file", &args, &spec, &mut errors);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, E1004);
        // Bug #8: never-clause errors explain that no allowlist overrides them.
        assert!(
            errors[0].message.contains("help:"),
            "no help line: {}",
            errors[0].message
        );
        assert!(
            errors[0].message.contains("remove"),
            "should suggest removal: {}",
            errors[0].message
        );
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
    fn non_literal_arg_with_nonempty_allowlist_fails_closed() {
        // SECURITY (was fail-OPEN): a dynamic path against a NON-EMPTY allowlist
        // used to be allowed ("runtime-deferred"), but @[contained] has no runtime
        // target check, so the write/read actually happened — a `@[contained(fs:
        // [read("./data/")])]` fn could `read_file(p)` ANY path. It now fails
        // CLOSED: an unverifiable dynamic target is refused (E1001) because it
        // could escape the allowlist (e.g. via `..`) and nothing enforces it at
        // runtime. (Holding SOME capability does not make an arbitrary target ok.)
        let spec = make_spec(vec!["./data/"], vec!["./out/"], vec![]);
        let args = vec![Expr::Ident("path".into())];

        let mut errors = Vec::new();
        check_call("read_file", &args, &spec, &mut errors);
        assert_eq!(
            errors.len(),
            1,
            "dynamic read against a non-empty allowlist must fail closed"
        );
        assert_eq!(errors[0].code, E1001);

        let mut errors = Vec::new();
        check_call("write_file", &args, &spec, &mut errors);
        assert_eq!(
            errors.len(),
            1,
            "dynamic write against a non-empty allowlist must fail closed"
        );
        assert_eq!(errors[0].code, E1001);

        // No false positive: a LITERAL path inside the allowlist still passes.
        let lit = vec![Expr::Literal(crate::ast::Literal::Str(
            "./data/x.txt".into(),
        ))];
        let mut errors = Vec::new();
        check_call("read_file", &lit, &spec, &mut errors);
        assert!(
            errors.is_empty(),
            "a literal in-allowlist read must still be permitted"
        );
    }

    #[test]
    fn dynamic_arg_with_empty_allowlist_fails_closed() {
        // The laundering-hole regression guard: a dynamic/interpolated target
        // against an EMPTY allowlist must be DENIED (zero capability → target
        // irrelevant). Covers read/write/net; exec is name-classified already.
        let empty = make_spec(vec![], vec![], vec![]);
        let dyn_arg = vec![Expr::Ident("p".into())];

        let mut e = Vec::new();
        check_call("read_file", &dyn_arg, &empty, &mut e);
        assert_eq!(e.len(), 1, "dynamic read_file under fs:[] must be denied");
        assert_eq!(e[0].code, E1001);

        let mut e = Vec::new();
        check_call("write_file", &dyn_arg, &empty, &mut e);
        assert_eq!(e.len(), 1, "dynamic write_file under fs:[] must be denied");

        let mut e = Vec::new();
        check_call("ai_complete", &dyn_arg, &empty, &mut e);
        assert_eq!(
            e.len(),
            1,
            "dynamic ai_complete under net:[] must be denied"
        );
        assert_eq!(e[0].code, E1001);
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
        assert!(
            errs[0].message.contains("net"),
            "names the widened cap: {}",
            errs[0].message
        );
        assert!(
            errs[0].message.contains("evil::net"),
            "names the import: {}",
            errs[0].message
        );
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
        assert!(
            errs.is_empty(),
            "read within grant must be allowed: {errs:?}"
        );
    }

    #[test]
    fn uncontained_importer_has_no_ceiling() {
        // BACK-COMPAT: an importer with NO @[contained] declares no boundary, so
        // importing a capability-exercising module is not a *widening* — E1203 is
        // opt-in. (This is why existing module-importing examples are unaffected.)
        let importer = parse("fn main() -> i64 { 0 }");
        let imported =
            parse("fn fetch() -> str { match http_get(\"api.x\") { Ok(s) => s  Err(_) => \"\" } }");
        let errs = check_import_capabilities(&importer, "any::net", &imported);
        assert!(
            errs.is_empty(),
            "uncontained importer has no ceiling to widen: {errs:?}"
        );
    }

    #[test]
    fn import_edge_sees_capability_inside_a_with_block() {
        // The import-edge capability walk (collect_caps_expr) must see into a
        // `with handler { … } { body }` — otherwise an imported module could
        // launder a capability past the importer's @[contained] ceiling by
        // performing it inside a `with` block. (collect_caps_expr had a `_ => {}`
        // catch-all that skipped WithHandler/Spawn/Select/Comptime; check_expr,
        // the self-check walker, already handled them.)
        let importer = parse(
            "@[contained(fs: [read(\"./data/\")], exec: none)]\n\
             fn main() -> i64 { 0 }",
        );
        // The imported module performs a NET call (http_get) inside a with-block —
        // net is outside the importer's fs-read-only ceiling, so it must widen.
        let imported = parse(
            "fn sneaky() -> str { with handler { on IO(p) => resume(0) } \
             { match http_get(\"api.evil.com\") { Ok(s) => s  Err(_) => \"\" } } }",
        );
        let errs = check_import_capabilities(&importer, "evil::hidden", &imported);
        assert_eq!(
            errs.len(),
            1,
            "net hidden in a with-block must still widen: {errs:?}"
        );
        assert_eq!(errs[0].code, E1203);
        assert!(
            errs[0].message.contains("net"),
            "must name the laundered net cap: {}",
            errs[0].message
        );
    }

    #[test]
    fn program_capabilities_sees_impl_method_bodies() {
        // BLIND-SPOT GUARD: program_capabilities (the surface R10's G2 self-
        // improvement gate diffs, AND the import-edge E1203 demand set) walked
        // ONLY Item::FnDef — so a capability call inside an `impl` method body
        // was INVISIBLE. A self-improving pass could move/introduce an exfil call
        // into a trait impl and G2 would see no capability widening; an imported
        // module could launder net/fs/exec past the importer's ceiling through an
        // impl method. This proves the walk now descends into ImplBlock methods.
        let p = parse(
            "type Sender = { id: i64 }\n\
             trait Exfil { fn leak(self) -> str }\n\
             impl Exfil for Sender {\n\
               fn leak(self: Sender) -> str {\n\
                 match http_get(\"api.evil.com\") { Ok(s) => s  Err(_) => \"\" }\n\
               }\n\
             }\n\
             fn main() -> i64 { 0 }",
        );
        let caps = program_capabilities(&p);
        assert!(
            caps.contains("net"),
            "net call inside an impl method must be visible to G2/import-edge, got: {caps:?}"
        );
    }

    #[test]
    fn impl_method_with_own_contained_is_enforced() {
        // The self-check (E1001) surface enforces a @[contained] declared ON an
        // impl method. Two fixes make this work end-to-end: (1) the PARSER now
        // extracts @[contained]/@[verify] specs for impl methods via the shared
        // `parse_attrs_with_specs` (parse_impl_block), and (2) check_capabilities
        // WALKS Item::ImplBlock methods. `ai_complete` is a net call
        // (classify_call), checked independent of return-type inference.
        let p = parse(
            "type T = { x: i64 }\n\
             trait N { fn go(self) -> str }\n\
             impl N for T {\n\
               @[contained(net: [], exec: none)]\n\
               fn go(self: T) -> str { ai_complete(\"exfil\") }\n\
             }",
        );
        let errs = check_capabilities(&p);
        assert!(
            errs.iter().any(|e| e.code == E1001),
            "contained impl method must be enforced (E1001): {errs:?}"
        );
    }

    #[test]
    fn contained_fn_cannot_launder_via_method_dispatch() {
        // THE method-dispatch gap (the last in this class): a @[contained] free
        // fn calls `t.go()`, and `go`'s impl exfiltrates. Previously the
        // MethodCall walk checked receiver+args but NEVER followed into the impl
        // body, so this laundered the exfil past the caller's sandbox. The walk
        // now follows dispatch (over-approximated by method name) into the body.
        let p = parse(
            "type T = { x: i64 }\n\
             trait Net { fn go(self) -> str }\n\
             impl Net for T {\n\
               fn go(self: T) -> str { ai_complete(\"exfil\") }\n\
             }\n\
             @[contained(net: [], exec: none)]\n\
             fn caller() -> i64 {\n\
               let t = T { x: 1 }\n\
               let _ = t.go()\n\
               0\n\
             }",
        );
        let errs = check_capabilities(&p);
        assert!(
            errs.iter().any(|e| e.code == E1001),
            "exfil via method dispatch must be caught under the caller's spec: {errs:?}"
        );
    }

    #[test]
    fn method_dispatch_within_grant_is_allowed() {
        // The ALLOW companion: a contained fn that grants net and calls a method
        // hitting an allowed host must NOT error — the dispatch-following must not
        // over-deny a call that's within the declared capability.
        let p = parse(
            "type T = { x: i64 }\n\
             trait Net { fn go(self) -> str }\n\
             impl Net for T {\n\
               fn go(self: T) -> str { match http_get(\"api.ok.com\") { Ok(s) => s  Err(_) => \"\" } }\n\
             }\n\
             @[contained(net: [\"api.ok.com\"], exec: none)]\n\
             fn caller() -> i64 {\n\
               let t = T { x: 1 }\n\
               let _ = t.go()\n\
               0\n\
             }",
        );
        let errs = check_capabilities(&p);
        assert!(
            errs.is_empty(),
            "method call within the granted net allowlist must be allowed: {errs:?}"
        );
    }

    #[test]
    fn method_dispatch_recursion_terminates() {
        // Cycle guard: a method that calls itself (or mutually-recursive methods)
        // via dispatch must not loop the walker. visited_methods stops it.
        let p = parse(
            "type T = { x: i64 }\n\
             trait R { fn rec(self) -> i64 }\n\
             impl R for T {\n\
               fn rec(self: T) -> i64 { let t = T { x: 0 }  t.rec() }\n\
             }\n\
             @[contained(net: [], exec: none)]\n\
             fn caller() -> i64 { let t = T { x: 1 }  t.rec() }",
        );
        // Must terminate (the assertion is that this returns at all); no I/O so
        // no errors expected.
        let errs = check_capabilities(&p);
        assert!(
            errs.is_empty(),
            "pure recursive method dispatch: no errors: {errs:?}"
        );
    }

    // ── Laundering through deferred / nested control forms ────────────────────
    // check_expr recurses into Lambda, Spawn, and WithHandler bodies; these guard
    // that a @[contained] fn cannot hide a forbidden I/O call inside one of them
    // (no false negative), and the companion that a GRANTED call inside the same
    // form is still allowed (no false positive). A future refactor that drops one
    // of those recursion arms would reopen a real laundering hole — these catch it.
    #[test]
    fn contained_fn_cannot_launder_via_lambda_body() {
        let p = parse(
            "@[contained(net: [], fs: [], exec: none)]\n\
             fn s() -> i64 { let f = || { match ai_complete(\"exfil\") { Ok(t) => len(t)  Err(_) => 0 } }  f() }\n\
             fn main() -> i64 { s() }",
        );
        let errs = check_capabilities(&p);
        assert!(
            errs.iter().any(|e| e.code == E1001),
            "a net call hidden in a lambda body must be caught under the fn's spec: {errs:?}"
        );
    }

    #[test]
    fn lambda_body_within_grant_is_allowed() {
        let p = parse(
            "@[contained(net: [\"api.anthropic.com\"], fs: [], exec: none)]\n\
             fn s() -> i64 { let f = || { match ai_complete(\"ok\") { Ok(t) => len(t)  Err(_) => 0 } }  f() }\n\
             fn main() -> i64 { s() }",
        );
        let errs = check_capabilities(&p);
        assert!(
            errs.is_empty(),
            "an ai_complete call in a lambda within the granted net host must be allowed: {errs:?}"
        );
    }

    #[test]
    fn contained_fn_cannot_launder_via_with_handler_body() {
        let p = parse(
            "@[contained(net: [], fs: [], exec: none)]\n\
             fn s() -> i64 { with handler { on IO(p) => resume(0) } { match ai_complete(\"exfil\") { Ok(t) => len(t)  Err(_) => 0 } } }\n\
             fn main() -> i64 { s() }",
        );
        let errs = check_capabilities(&p);
        assert!(
            errs.iter().any(|e| e.code == E1001),
            "a net call hidden in a with-handler body must be caught under the fn's spec: {errs:?}"
        );
    }

    #[test]
    fn contained_fn_cannot_launder_via_spawn_body() {
        let p = parse(
            "@[contained(net: [], fs: [], exec: none)]\n\
             fn s() -> i64 { spawn { match ai_complete(\"exfil\") { Ok(t) => len(t)  Err(_) => 0 } }  0 }\n\
             fn main() -> i64 { s() }",
        );
        let errs = check_capabilities(&p);
        assert!(
            errs.iter().any(|e| e.code == E1001),
            "a net call hidden in a spawn body must be caught under the fn's spec: {errs:?}"
        );
    }

    // ── env_var: ungrantable ambient secret channel inside @[contained] ───────
    #[test]
    fn contained_fn_cannot_read_env_var() {
        // The exfil hole: a sandboxed fn that grants only a specific net host can
        // still read host secrets (API keys/tokens) from the environment and leak
        // them via the allowed channel — defeating containment. env reads have no
        // grant clause, so they're denied inside @[contained] (fail closed).
        let p = parse(
            "@[contained(net: [\"api.allowed.com\"], fs: [], exec: none)]\n\
             fn evil() -> str { match env_var(\"SECRET_KEY\") { Ok(v) => v  Err(_) => \"\" } }\n\
             fn main() -> i64 { len(evil()) }",
        );
        let errs = check_capabilities(&p);
        assert!(
            errs.iter()
                .any(|e| e.code == E1001 && e.message.contains("environment")),
            "reading env inside @[contained] must be denied (no grant clause exists): {errs:?}"
        );
    }

    #[test]
    fn env_var_outside_contained_is_allowed() {
        // No false positive: env reads in UNCONTAINED code are unrestricted —
        // containment is opt-in, and a program that declares no boundary keeps
        // full ambient access.
        let p =
            parse("fn main() -> i64 { match env_var(\"HOME\") { Ok(v) => len(v)  Err(_) => 0 } }");
        let errs = check_capabilities(&p);
        assert!(
            errs.is_empty(),
            "env_var outside @[contained] must be allowed: {errs:?}"
        );
    }

    #[test]
    fn env_read_outside_then_passed_into_contained_fn_is_allowed() {
        // The sanctioned pattern the deny steers toward: read the env in the
        // (uncontained) caller and hand the value to the contained fn explicitly,
        // so the sandboxed code only sees what it was given.
        let p = parse(
            "@[contained(net: [], fs: [], exec: none)]\n\
             fn process(cfg: str) -> i64 { len(cfg) }\n\
             fn main() -> i64 {\n\
               let c = match env_var(\"CONFIG\") { Ok(v) => v  Err(_) => \"\" }\n\
               process(c)\n\
             }",
        );
        let errs = check_capabilities(&p);
        assert!(errs.is_empty(),
            "reading env outside and passing the value into a contained fn must be allowed: {errs:?}");
    }

    #[test]
    fn imported_env_read_widens_a_contained_importer_e1203() {
        // The import-edge consequence: env has no grant clause, so it's never in
        // the importer's ceiling — an imported module that reads the environment
        // widens a @[contained] importer's surface (E1203), fail-closed.
        let importer = parse(
            "@[contained(fs: [read(\"./data/\")], exec: none)]\n\
             fn main() -> i64 { 0 }",
        );
        let imported =
            parse("fn peek() -> str { match env_var(\"SECRET\") { Ok(v) => v  Err(_) => \"\" } }");
        let errs = check_import_capabilities(&importer, "mod::peek", &imported);
        assert!(
            errs.iter()
                .any(|e| e.code == E1203 && e.message.contains("env")),
            "an imported env read must widen the contained importer's ceiling: {errs:?}"
        );
    }

    #[test]
    fn impl_method_cannot_launder_capability_past_import_ceiling() {
        // The end-to-end consequence: an importer contained to fs-read only must
        // still reject an imported module that hits the network FROM AN IMPL
        // METHOD (the laundering vector the blind spot opened).
        let importer = parse(
            "@[contained(fs: [read(\"./data/\")], exec: none)]\n\
             fn main() -> i64 { 0 }",
        );
        let imported = parse(
            "type T = { x: i64 }\n\
             trait Net { fn go(self) -> str }\n\
             impl Net for T {\n\
               fn go(self: T) -> str { match http_get(\"api.evil.com\") { Ok(s) => s  Err(_) => \"\" } }\n\
             }",
        );
        let errs = check_import_capabilities(&importer, "evil::impl", &imported);
        assert_eq!(
            errs.len(),
            1,
            "net hidden in an impl method must widen: {errs:?}"
        );
        assert_eq!(errs[0].code, E1203);
        assert!(
            errs[0].message.contains("net"),
            "must name the laundered net cap: {}",
            errs[0].message
        );
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
        assert!(
            errs[0].message.contains("fs:read"),
            "first: {}",
            errs[0].message
        );
        assert!(
            errs[1].message.contains("net"),
            "second: {}",
            errs[1].message
        );
    }
}
