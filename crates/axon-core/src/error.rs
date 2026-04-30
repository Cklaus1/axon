//! Canonical diagnostic type for the Axon compiler.
//!
//! Both the resolver and checker define local error structs while this module
//! was being written in parallel.  Future work: migrate those to import from
//! here once the dependency order is settled.

// ── Error codes ───────────────────────────────────────────────────────────────

// Inference / type errors
pub const E0101: &str = "E0101";
pub const E0102: &str = "E0102";

// Resolution errors
pub const E0001: &str = "E0001";
pub const E0002: &str = "E0002";
pub const E0003: &str = "E0003";

// Type-check errors
pub const E0301: &str = "E0301";
pub const E0302: &str = "E0302";
pub const E0303: &str = "E0303";
pub const E0304: &str = "E0304";
pub const E0305: &str = "E0305";
pub const E0306: &str = "E0306";
pub const E0307: &str = "E0307";
pub const E0308: &str = "E0308";
pub const E0309: &str = "E0309";
// R09: unused variable
pub const E0310: &str = "E0310";
// R10: dead code after return
pub const E0311: &str = "E0311";
// R11: non-exhaustive match on user enum
pub const E0312: &str = "E0312";
// R12: calling a non-function value
pub const E0313: &str = "E0313";
// Arithmetic on non-numeric type
pub const E0314: &str = "E0314";
// Assignment type mismatch
pub const E0315: &str = "E0315";

// Field / struct errors
pub const E0401: &str = "E0401"; // struct has no field

// Trait errors (Phase 3)
pub const E0501: &str = "E0501"; // trait method not implemented
pub const E0502: &str = "E0502"; // impl block missing method
pub const E0503: &str = "E0503"; // dyn trait cannot be used as value type
pub const E0504: &str = "E0504"; // trait bound not satisfied

// Borrow errors (Phase 3)
pub const E0601: &str = "E0601"; // use of moved value
pub const E0602: &str = "E0602"; // cannot move borrowed value
pub const E0603: &str = "E0603"; // borrow conflict

// Comptime errors (Phase 3)
pub const E0701: &str = "E0701"; // expression not comptime-evaluable
pub const E0702: &str = "E0702"; // comptime integer division by zero
pub const E0703: &str = "E0703"; // comptime integer overflow

// Generics errors (Phase 3)
pub const E0801: &str = "E0801"; // generic instantiation depth exceeded
pub const E0802: &str = "E0802"; // cannot infer type argument
pub const E0803: &str = "E0803"; // type argument does not satisfy bound

// Multi-file compilation errors (Phase 4)
pub const E0901: &str = "E0901"; // module not found (AXON_PATH search failed)
pub const E0902: &str = "E0902"; // circular import between source files
pub const E0903: &str = "E0903"; // duplicate top-level name across files
pub const E0904: &str = "E0904"; // --target triple not supported by this LLVM build
pub const E0905: &str = "E0905"; // cross-compilation needs sysroot (cross.toml missing)
pub const E0906: &str = "E0906"; // cache entry corrupt or wrong compiler version

// Capability permission errors (Phase 4: @[contained])
pub const E1001: &str = "E1001"; // I/O call not permitted by @[contained] spec
pub const E1002: &str = "E1002"; // @[contained] clause is malformed
pub const E1003: &str = "E1003"; // capability path is not parseable
pub const E1004: &str = "E1004"; // call hits a never: clause (hard violation)

// Warning codes
pub const W0001: &str = "W0001"; // unknown attribute
pub const W0002: &str = "W0002"; // variable shadowing
// Layer-1 ASI warnings
pub const W0701: &str = "W0701"; // uncertainty discarded (Uncertain<T>.value used without checking .confidence)

// Info codes
pub const I0001: &str = "I0001"; // deferred attribute (AI annotations)

// ── Severity ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

// ── AxonError ─────────────────────────────────────────────────────────────────

/// A compiler diagnostic — emitted by the resolver, inference engine, and
/// type checker. Serialises to both JSON (for AI tooling) and ANSI-coloured
/// text (for human terminals).
#[derive(Debug, Clone)]
pub struct AxonError {
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
}

impl AxonError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        AxonError {
            code,
            message: message.into(),
            node_id: String::new(),
            file: String::new(),
            line: 0,
            col: 0,
            expected: None,
            found: None,
            fix: None,
            severity: Severity::Error,
        }
    }

    pub fn info(code: &'static str, message: impl Into<String>) -> Self {
        let mut e = Self::new(code, message);
        e.severity = Severity::Info;
        e
    }

    pub fn warning(code: &'static str, message: impl Into<String>) -> Self {
        let mut e = Self::new(code, message);
        e.severity = Severity::Warning;
        e
    }

    pub fn at(mut self, file: impl Into<String>, line: u32, col: u32) -> Self {
        self.file = file.into();
        self.line = line;
        self.col = col;
        self
    }

    pub fn node(mut self, id: impl Into<String>) -> Self {
        self.node_id = id.into();
        self
    }

    pub fn expected(mut self, e: impl Into<String>) -> Self {
        self.expected = Some(e.into());
        self
    }

    pub fn found(mut self, f: impl Into<String>) -> Self {
        self.found = Some(f.into());
        self
    }

    pub fn fix(mut self, f: impl Into<String>) -> Self {
        self.fix = Some(f.into());
        self
    }

    /// Format as a human-readable one-liner (no ANSI colours).
    pub fn display(&self) -> String {
        let loc = if self.line > 0 {
            format!("{}:{}:{}: ", self.file, self.line, self.col)
        } else if !self.file.is_empty() {
            format!("{}: ", self.file)
        } else {
            String::new()
        };
        let prefix = match self.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "note",
        };
        let mut s = format!("{loc}{prefix}[{}]: {}", self.code, self.message);
        if let Some(exp) = &self.expected {
            s.push_str(&format!("\n  expected: {exp}"));
        }
        if let Some(found) = &self.found {
            s.push_str(&format!("\n     found: {found}"));
        }
        if let Some(fix) = &self.fix {
            s.push_str(&format!("\n       fix: {fix}"));
        }
        s
    }
}

// ── Levenshtein distance ──────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_codes_are_unique() {
        let codes = [
            E0001, E0002, E0003,
            E0101, E0102,
            E0301, E0302, E0303, E0304, E0305,
            E0306, E0307, E0308, E0309, E0310,
            E0311, E0312, E0313, E0314, E0315,
            E0401,
            E0501, E0502, E0503, E0504,
            E0601, E0602, E0603,
            E0701, E0702, E0703,
            E0801, E0802, E0803,
            E0901, E0902, E0903, E0904, E0905, E0906,
            E1001, E1002, E1003, E1004,
            W0001, W0002, W0701,
            I0001,
        ];
        let mut seen = std::collections::HashSet::new();
        for code in &codes {
            assert!(seen.insert(*code), "duplicate error code: {code}");
        }
    }

    #[test]
    fn axon_error_display_basic() {
        let e = AxonError::new(E0001, "undefined variable 'foo'");
        let d = e.display();
        assert!(d.contains("E0001"));
        assert!(d.contains("foo"));
        assert!(d.contains("error"));
    }

    #[test]
    fn axon_error_display_with_location() {
        let e = AxonError::new(E0001, "undefined variable")
            .at("main.ax", 5, 10);
        let d = e.display();
        assert!(d.contains("main.ax:5:10"));
    }

    #[test]
    fn axon_error_display_with_expected_found() {
        let e = AxonError::new(E0102, "type mismatch")
            .expected("i64")
            .found("str");
        let d = e.display();
        assert!(d.contains("expected: i64"));
        assert!(d.contains("found: str"));
    }

    #[test]
    fn axon_error_severity_prefix() {
        let err = AxonError::new(E0001, "msg");
        assert!(err.display().contains("error"));

        let warn = AxonError::warning(W0001, "msg");
        assert!(warn.display().contains("warning"));

        let info = AxonError::info(I0001, "msg");
        assert!(info.display().contains("note"));
    }

    #[test]
    fn axon_error_fix_shown() {
        let e = AxonError::new(E0001, "undefined")
            .fix("did you mean 'foo'?");
        let d = e.display();
        assert!(d.contains("fix:"));
        assert!(d.contains("foo"));
    }
}

/// Classic Wagner-Fischer Levenshtein distance using rolling-row DP.
/// Returns early when the distance exceeds `cutoff` (default 3) to avoid
/// allocating large matrices for clearly-unrelated names.
pub fn levenshtein(a: &str, b: &str) -> usize {
    const CUTOFF: usize = 3;
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > CUTOFF {
        return CUTOFF + 1;
    }
    let m = a.len();
    let n = b.len();
    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];
    for i in 1..=m {
        curr[0] = i;
        for j in 1..=n {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}
