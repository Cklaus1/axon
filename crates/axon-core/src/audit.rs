//! R6 §4.3 — capability audit on import.
//!
//! When a module is acquired (`axon lock`/`add`), its capability surface is
//! audited and the **verdict** is pinned into `axon.lock` (the `audit` field).
//! On every subsequent build the verdict is re-validated cheaply by hash — no
//! AI call at compile time (R6 §4.3 "acquisition-time audit, hash-pinned
//! verdict"). A `denied` verdict fails the build with **E1204**.
//!
//! ## What this slice audits (and what it honestly does not)
//!
//! The audit here is the **deterministic, static** half: it inspects the
//! imported module's *exercised* capabilities (`program_capabilities`) against
//! its *declared* `@[contained]` surface and produces a verdict:
//!
//! | Verdict   | Condition |
//! |-----------|-----------|
//! | `clear`   | the module exercises no capability, OR it declares a `@[contained]` (it has acknowledged its surface; the static checker then enforces it) |
//! | `flagged` | the module exercises **only** undeclared `fs` (read/write) surface — a human should review (`--accept-flagged`) |
//! | `denied`  | the module exercises undeclared **`net`** — the exfiltration channel; a dependency that phones home with no declared containment is the canonical supply-chain risk, blocked by default (E1204) |
//!
//! **Why `net` is the `denied` tier (a deviation from the spec's `exec`
//! example, stated honestly):** the spec (R6 §4.3) names undeclared `exec` as
//! the highest-risk surface, but Axon has **no `exec` builtin yet** — nothing
//! in `classify_call` maps to `IoKind::Exec`, so an exec-based denied tier
//! would be untestable dead code. Undeclared `net` *is* detectable today
//! (`ai_complete`/`http_get`) and is the real exfiltration channel a
//! capability audit exists to catch, so this slice keys `denied` on it. When an
//! `exec` builtin lands, it joins the `denied` tier.
//!
//! This is **defense-in-depth, not a proof** (R6 §4.3 "Honest limit"). The hard
//! guarantee remains the static capability checker (`capabilities.rs`,
//! E1001–E1004) on the merged program — the TCB-grade boundary. The eventual
//! AI-audit (an `ai_complete` call that reasons about obfuscation/intent, R3)
//! layers *on top* of this deterministic floor; it is a future enrichment, and
//! deliberately not wired here so `axon lock` stays offline + deterministic.
//!
//! The verdict is deterministic in the source bytes, so it can be pinned and
//! re-validated by hash (R6 §4.6).

use crate::ast::{Item, Program};

/// A capability-audit verdict for one imported module. Ordered by severity so a
/// `match` reads worst-last; `as_str`/`parse` round-trip the lockfile token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// No undeclared capability surface — safe to import with one hash compare.
    Clear,
    /// Undeclared `fs`-only surface — a human should review (`--accept-flagged`).
    Flagged,
    /// Undeclared `net` (exfiltration channel) — blocked by default (E1204).
    Denied,
}

impl Verdict {
    /// The lockfile token for this verdict (`clear`/`flagged`/`denied`).
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Clear => "clear",
            Verdict::Flagged => "flagged",
            Verdict::Denied => "denied",
        }
    }

    /// Parse a lockfile token back into a verdict; unknown tokens are treated as
    /// `Flagged` (conservative — an unrecognised verdict is never silently
    /// `clear`).
    pub fn parse(s: &str) -> Verdict {
        match s {
            "clear" => Verdict::Clear,
            "denied" => Verdict::Denied,
            _ => Verdict::Flagged,
        }
    }
}

/// Does any fn in `program` declare a `@[contained]`?
fn any_contained(program: &Program) -> bool {
    program.items.iter().any(|it| matches!(it, Item::FnDef(f) if f.contained.is_some()))
}

/// Audit a parsed imported module and return its [`Verdict`] (R6 §4.3).
///
/// Deterministic in the module's AST: undeclared `net` → `Denied`; any other
/// undeclared capability (`fs`) → `Flagged`; otherwise `Clear`. A module that
/// declares its own `@[contained]` has *acknowledged* its surface, so it is
/// `Clear` even if it exercises I/O — the static checker (E1001–E1004) then
/// enforces those declarations on the merged program.
pub fn audit_module(program: &Program) -> Verdict {
    let caps = crate::capabilities::program_capabilities(program);
    if caps.is_empty() {
        return Verdict::Clear;
    }
    // A module that declares containment has acknowledged its surface; the
    // static checker enforces it. Undeclared surface is what the audit flags.
    if any_contained(program) {
        return Verdict::Clear;
    }
    // Undeclared `net` is the exfiltration channel — denied by default.
    // (An `exec` builtin, when it lands, joins this tier.)
    if caps.iter().any(|c| c == "net" || c == "exec") {
        return Verdict::Denied;
    }
    Verdict::Flagged
}

/// A one-line, human-readable reason for a non-`clear` verdict — surfaced in the
/// E1204 message and the `axon audit` report.
pub fn verdict_reason(program: &Program, verdict: Verdict) -> String {
    let caps = crate::capabilities::program_capabilities(program);
    let cap_list = caps.iter().cloned().collect::<Vec<_>>().join(", ");
    match verdict {
        Verdict::Clear => "no undeclared capability surface".to_string(),
        Verdict::Flagged => format!("undeclared capability surface: {cap_list}"),
        Verdict::Denied => format!("undeclared `exec` (full surface: {cap_list})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_source;

    fn audit_src(src: &str) -> Verdict {
        let p = parse_source(src).expect("parse");
        audit_module(&p)
    }

    #[test]
    fn pure_module_is_clear() {
        assert_eq!(audit_src("fn score(x: i64) -> i64 { x * 2 }\n"), Verdict::Clear);
    }

    #[test]
    fn undeclared_net_is_denied() {
        // A module that makes a network call (the exfiltration channel) with no
        // @[contained] is the highest-risk *detectable* surface → Denied (E1204).
        let v = audit_src("fn fetch() -> str { match ai_complete(\"x\") { Ok(s) => s  Err(_) => \"\" } }\n");
        assert_eq!(v, Verdict::Denied, "undeclared net must be denied");
    }

    #[test]
    fn undeclared_fs_is_flagged() {
        let v = audit_src("fn load(p: str) -> str { read_file(p) }\n");
        assert_eq!(v, Verdict::Flagged, "undeclared fs-only surface must be flagged");
    }

    #[test]
    fn declared_containment_is_clear() {
        // The module acknowledges its surface via @[contained]; the static
        // checker enforces it, so the audit is Clear (not flagged).
        let v = audit_src(
            "@[contained(net: [\"api.example.com\"])]\nfn fetch() -> str { match ai_complete(\"x\") { Ok(s) => s  Err(_) => \"\" } }\n",
        );
        assert_eq!(v, Verdict::Clear, "declared containment → clear");
    }

    #[test]
    fn verdict_token_roundtrips() {
        for v in [Verdict::Clear, Verdict::Flagged, Verdict::Denied] {
            assert_eq!(Verdict::parse(v.as_str()), v);
        }
        // Unknown token is conservatively Flagged, never Clear.
        assert_eq!(Verdict::parse("garbage"), Verdict::Flagged);
    }

    #[test]
    fn audit_is_deterministic() {
        let src = "fn run() -> i64 { exec(\"ls\") }\n";
        assert_eq!(audit_src(src), audit_src(src));
    }
}
