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
pub const E0907: &str = "E0907"; // AOT wasm build needs the native codegen backend (R7)
pub const E0908: &str = "E0908"; // no engine supports the requested target triple (R7, reserved)

// Capability permission errors (Phase 4: @[contained])
pub const E1001: &str = "E1001"; // I/O call not permitted by @[contained] spec
pub const E1002: &str = "E1002"; // @[contained] clause is malformed
pub const E1003: &str = "E1003"; // capability path is not parseable
pub const E1004: &str = "E1004"; // call hits a never: clause (hard violation)

// Verify errors (ASI Layer-2: @[verify])
pub const E1101: &str = "E1101"; // verify bound not satisfied (runtime gate)
pub const E1102: &str = "E1102"; // verify bound statically VIOLABLE — SMT counterexample (R9)

// Capability/registry security errors (R6: content-addressed imports, axon.lock)
pub const E1201: &str = "E1201"; // on-disk module bytes ≠ axon.lock hash (tamper)
pub const E1202: &str = "E1202"; // use with no lockfile entry under --locked
pub const E1203: &str = "E1203"; // import declares capabilities beyond the importer's grant
pub const E1204: &str = "E1204"; // lockfile audit verdict is `denied`
pub const E1205: &str = "E1205"; // axon.lock is malformed / unknown version

// AI-primitive errors (R3: ai_complete / @[ai(policy)])
pub const E1300: &str = "E1300"; // ai_* call unreachable and no @[ai(policy(fallback))] in scope
pub const E1301: &str = "E1301"; // ai_complete exceeded the fn's @[ai(policy(budget: N))] (R3c)
pub const E1302: &str = "E1302"; // tier: resolves to a tier with no host-configured model

// Self-improving-compiler errors (R10: pass verification harness)
pub const E1401: &str = "E1401"; // G1 correctness: pass changes observable output on a corpus member
pub const E1402: &str = "E1402"; // G2 safety: pass adds a capability the original lacked (I-12)
pub const E1403: &str = "E1403"; // G3 regression: pass breaks an existing test
pub const E1404: &str = "E1404"; // graduation requires multi-sig of root Principals (I-12)
pub const E1405: &str = "E1405"; // pass manifest hash mismatch at boot (TCB attestation)
pub const E1406: &str = "E1406"; // correctness judged by AI — forbidden; the oracle is the interpreter

// R5 goal sugar: #[goal(...)] attribute diagnostics
pub const E1500: &str = "E1500"; // metric must name an @[adaptive] fn
pub const E1503: &str = "E1503"; // target/max_evals/holdout must parse as numbers
pub const E1504: &str = "E1504"; // #[goal] fn must have zero params

// Warning codes
pub const W0001: &str = "W0001"; // unknown attribute
pub const W0002: &str = "W0002"; // variable shadowing
pub const W0003: &str = "W0003"; // user fn shadows a builtin (builtin takes precedence)
// Layer-1 ASI warnings
pub const W0701: &str = "W0701"; // uncertainty discarded (Uncertain<T>.value used without checking .confidence)
pub const W1103: &str = "W1103"; // @[verify] outside the SMT-provable fragment (R9); runtime gate applies
// R3c AI-budget warning
pub const W1311: &str = "W1311"; // @[ai(policy(budget: N))] value is not a non-negative integer; ignored
// R6 registry warnings
pub const W1210: &str = "W1210"; // use resolved by AXON_PATH with no lockfile entry (dev mode, unaudited)
// R10 self-improving warnings
pub const W1410: &str = "W1410"; // pass claims `faster` but the perf gate (G4) was not run
// R3 AI-primitive warning
pub const W1310: &str = "W1310"; // live AI call by a fn with no @[ai(policy)] (un-metered/un-pinned)

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
            E0901, E0902, E0903, E0904, E0905, E0906, E0907, E0908,
            E1001, E1002, E1003, E1004,
            E1101, E1102,
            E1201, E1202, E1203, E1204, E1205,
            E1300, E1301, E1302,
            E1401, E1402, E1403, E1404, E1405, E1406,
            E1500, E1503, E1504,
            W0001, W0002, W0003, W0701, W1103, W1210, W1310, W1311, W1410,
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
