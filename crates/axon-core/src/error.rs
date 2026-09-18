//! Canonical diagnostic type for the Axon compiler.
//!
//! Both the resolver and checker define local error structs while this module
//! was being written in parallel.  Future work: migrate those to import from
//! here once the dependency order is settled.

// ── Error codes ───────────────────────────────────────────────────────────────

// Inference / type errors
pub const E0101: &str = "E0101";
pub const E0102: &str = "E0102";

// Generic / fallback
pub const E0000: &str = "E0000"; // generic parse/IO error with no more-specific code (CLI + LSP)

// Resolution errors
pub const E0001: &str = "E0001";
pub const E0002: &str = "E0002";
pub const E0003: &str = "E0003";
pub const E0004: &str = "E0004"; // reserved (Phase 2): use of a non-exported item across modules

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
pub const E0401: &str = "E0401"; // a field name that does not exist on the thing it is used with
pub const E0402: &str = "E0402"; // an index that cannot be valid
pub const E0403: &str = "E0403"; // calling a data field as a method (`p.x()`)
pub const E0404: &str = "E0404"; // a path names something absent, or the wrong kind of thing
pub const E0405: &str = "E0405"; // literal pattern's type can't match the match subject
pub const E0406: &str = "E0406"; // a field is set more than once in a struct literal
pub const E0407: &str = "E0407"; // integer division/remainder by a literal zero

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
pub const E0800: &str = "E0800"; // LSP: source could not be parsed (document-level diagnostic)
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
pub const E0910: &str = "E0910"; // builtin / construct has no native codegen lowering — honest abort, runs under the interpreter
pub const E0911: &str = "E0911"; // browser target (--host browser): a browser-incompatible builtin can't run in the tab — clean refusal mirroring E0910 (R7c)
pub const E0912: &str = "E0912"; // browser AOT link failed (wasm-bindgen / export step) (R7c, reserved)
pub const W0913: &str = "W0913"; // sleep_ms is a no-op on the browser host (main thread can't block) (R7c)

// Capability permission errors (Phase 4: @[contained])
pub const E1001: &str = "E1001"; // I/O call not permitted by @[contained] spec
pub const E1002: &str = "E1002"; // @[contained] clause is malformed
pub const E1003: &str = "E1003"; // capability path is not parseable
pub const E1004: &str = "E1004"; // call hits a never: clause (hard violation)
pub const E1005: &str = "E1005"; // R45: un-granted capability reached from an entry point under --require-contained

// Verify errors (ASI Layer-2: @[verify])
pub const E1101: &str = "E1101"; // verify bound not satisfied (runtime gate)
pub const E1102: &str = "E1102"; // verify bound statically VIOLABLE — SMT counterexample (R9)

// Capability/registry security errors (R6: content-addressed imports, axon.lock)
pub const E1201: &str = "E1201"; // on-disk module bytes ≠ axon.lock hash (tamper)
pub const E1202: &str = "E1202"; // use with no lockfile entry under --locked
pub const E1203: &str = "E1203"; // import declares capabilities beyond the importer's grant
pub const E1204: &str = "E1204"; // lockfile audit verdict is `denied`
pub const E1205: &str = "E1205"; // axon.lock is malformed / unknown version
pub const E1206: &str = "E1206"; // a @[sensitive] value flows into an external AI call (PRD §4 privacy)
pub const E1207: &str = "E1207"; // a @[pure] function performs/contains an impure operation (Phase 5 §2, P01/P02)
pub const E1208: &str = "E1208"; // a @[total] function has no strictly-decreasing measure at a recursive call (Phase 5 §3)
pub const E1209: &str = "E1209"; // a constant argument provably violates a refinement-type predicate (Phase 5 §1 R02)
pub const E1210: &str = "E1210"; // a `sql_query` template is not a string literal — user data must be a bound parameter, never concatenated SQL (injection)

// AI-primitive errors (R3: ai_complete / @[ai(policy)])
pub const E1300: &str = "E1300"; // ai_* call unreachable and no @[ai(policy(fallback))] in scope
pub const E1301: &str = "E1301"; // ai_complete exceeded the fn's @[ai(policy(budget: N))] (R3c)
pub const E1302: &str = "E1302"; // tier: resolves to a tier with no host-configured model
pub const E1303: &str = "E1303"; // run exceeded AXON_BUDGET_TOKENS, the ambient run-level token cap

// Phase 6 effect-row errors. The spec's nominal E1300-E1308 numbering collided
// with the AI-policy codes above (E1300-E1302), so effect-row diagnostics use
// the free E1310 block. E1310 covers spec rules E02 (subsumption) and E05 (a
// pure fn calling something effectful): a call performs an effect the enclosing
// fn's declared row does not contain.
pub const E1306: &str = "E1306"; // raw effect-row syntax `| {…}` used in a `surface`-marked file
pub const E1310: &str = "E1310"; // effect-row leak: call performs effect E ∉ the caller's declared row
pub const E1316: &str = "E1316"; // @[contained(...)] deprecation notice; prefer `| {…}` effect-row syntax

// Kernel TCB obligation errors (R20: SMT-proven capability primitives — E16xx band)
pub const E1610: &str = "E1610"; // a kernel capability-mint obligation (attenuation / budget-carve) is not SMT-discharged — the minter has been weakened (I-12 tripwire)
pub const E1611: &str = "E1611"; // TCB attestation mismatch at boot: the kernel obligation digest ≠ the pinned manifest (the proven TCB changed without a manifest update — I-12)

// Self-improving-compiler errors (R10: pass verification harness)
pub const E1401: &str = "E1401"; // G1 correctness: pass changes observable output on a corpus member
pub const E1402: &str = "E1402"; // G2 safety: pass adds a capability the original lacked (I-12)
pub const E1403: &str = "E1403"; // G3 regression: pass breaks an existing test
pub const E1404: &str = "E1404"; // graduation requires multi-sig of root Principals (I-12)
pub const E1405: &str = "E1405"; // pass manifest hash mismatch at boot (TCB attestation)
pub const E1406: &str = "E1406"; // correctness judged by AI — forbidden; the oracle is the interpreter
pub const E1407: &str = "E1407"; // AI proposed a template name not in the closed registry (rejected before verify)
pub const E1408: &str = "E1408"; // a graduated/verified pass name is absent from the template registry (tamper / version skew)
                                 // R10 Layer 3 — AI-authored RewriteSpec proposal-stage validation (fail-closed, before the firewall)
pub const E1409: &str = "E1409"; // RewriteSpec empty / not provably total (proposes no transform)
pub const E1411: &str = "E1411"; // RewriteSpec rule name outside the closed reviewed vocabulary
pub const E1412: &str = "E1412"; // RewriteSpec could express a capability (grammar violation — defense in design)
pub const E1413: &str = "E1413"; // RewriteSpec over the rule-count budget (runaway proposal rejected, never run)

// R5 goal sugar: #[goal(...)] attribute diagnostics
pub const E1500: &str = "E1500"; // metric must name an @[adaptive] fn
pub const E1503: &str = "E1503"; // target/max_evals/holdout must parse as numbers
pub const E1504: &str = "E1504"; // #[goal] fn must have zero params
pub const E1505: &str = "E1505"; // unknown #[goal(strategy: …)] — not hill_climb|random|multistart|tournament|bayesian
pub const E1900: &str = "E1900"; // R19: integer literal out of range for its fixed-width/unsigned annotation

// R42 stdlib gaps. E2200 appears in a runtime PANIC message and E2201/E2202/E2204
// as prefixes inside `Err` strings — every JSON/encoding builtin returns `Result`,
// so those failures are VALUES, not diagnostics, and there is no diagnostic for a
// code to attach to. They are registered here anyway so the code space stays
// single-sourced and nothing else can claim them.
pub const E2200: &str = "E2200"; // str_slice byte range splits a UTF-8 character (runtime panic text)
pub const E2201: &str = "E2201"; // malformed JSON where a document was required (Err-string prefix)
pub const E2202: &str = "E2202"; // JSON type mismatch at a path/element (Err-string prefix)
pub const E2203: &str = "E2203"; // regex construct requiring backtracking, or a pattern too large once counted repetitions expand
pub const E2204: &str = "E2204"; // base64/hex decode: invalid input, or bytes that are not valid UTF-8
pub const E2205: &str = "E2205"; // re_replace_all: replacement references a capture group the pattern does not have

// R17 freestanding substrate / HAL errors
pub const E1700: &str = "E1700"; // raw pointer `*T`, volatile_*, ptr_from_addr, or @[hal] used in a `surface` file
pub const E1701: &str = "E1701"; // @[hal] fn body calls hardware primitive without the Hal capability minted to its Principal
pub const E1702: &str = "E1702"; // freestanding build has no @[entry] or @[panic_handler]
pub const E1703: &str = "E1703"; // surface caller reaches a Hal-effected fn without declaring | {Hal}
pub const E1704: &str = "E1704"; // @[no_alloc] fn reaches a heap-allocating builtin (ISR/early-boot alloc-free guarantee)
pub const E1706: &str = "E1706"; // R17 Slice 2: atomic ordering arg is not a compile-time literal in 0..=4
pub const E1707: &str = "E1707"; // R17 §12 Q7: fn_addr's argument is not a compile-time string literal, or names no known function

// R14 mobile-target errors (spec §6). Allocated from the free tail of the E17xx
// band (E1700–E1706 are R17's, E1707 claimed 2026-07-20 for `fn_addr`; E1708–E1709
// left as a gap). Mobile reuses E1004
// (ungranted native::platform/gfx) and the R13 E18xx FFI codes for boundary errors.
pub const E1710: &str = "E1710"; // --host mobile but the mobile toolchain is absent: the Android NDK (linker on PATH) for an *-android triple, or Xcode `xcrun` for an *-apple-ios triple
pub const E1711: &str = "E1711"; // axon-rt / a native module not cross-built for the device triple (iOS-specific staging code)
pub const E1712: &str = "E1712"; // mobile cross-link failed (iOS xcframework or Android jniLibs packaging)

// R13 native FFI errors. The spec proposed E1700–E1704, but R17 (just landed)
// took the whole E170x band; R13 uses the free E18xx band instead. The spec's
// §6 table is updated to these codes in the docs commit. Use-after-consume of a
// resource Handle reuses the existing borrow-checker E0601 "use after move";
// capability-denied import reuses E1004; effect-not-declared reuses E1310.
pub const E1800: &str = "E1800"; // `use native::M` for an unregistered module name
pub const E1801: &str = "E1801"; // native call arg/ret type not FFI-representable
pub const E1802: &str = "E1802"; // handle of module A passed where module B's handle expected
pub const E1803: &str = "E1803"; // arithmetic / forging on a `Handle` (opaque, unconstructable)

// R24 TEE / confidential-computing errors. The Secret info-flow lattice composes
// with the enclave boundary: a sealed Secret may only be DECLASSIFIED (unsealed)
// inside an `@[enclave]` fn. This is a pure type/checker rule — enforceable on a
// host with NO TEE hardware. (Hardware attestation is produced only on a
// confidential runner; see governance/specs/R24-tee-target.md.)
pub const E1810: &str = "E1810"; // `tee_unseal` (Secret declassification) called outside an `@[enclave]` fn

// R23 eBPF target errors.
pub const E2300: &str = "E2300"; // a BPF helper not on the Axon capability allowlist is called from a @[bpf] program
pub const E2301: &str = "E2301"; // a construct outside the BPF-lowerable subset appears in a @[bpf] body
pub const E2302: &str = "E2302"; // @[bpf(kind: K)] has an unknown program kind

// R44 — accumulating typed session (E24xx band)
pub const E2400: &str = "E2400"; // redefining a name in a session breaks an item an earlier cell wrote
pub const E2402: &str = "E2402"; // a session cell is not a usable fragment (today: it declares its own `fn main`)
pub const E2403: &str = "E2403"; // malformed `axon session --protocol jsonl` input frame

// Warning codes
pub const W0001: &str = "W0001"; // unknown attribute
pub const W0002: &str = "W0002"; // variable shadowing
pub const W0003: &str = "W0003"; // user fn shadows a builtin (builtin takes precedence)
pub const W0004: &str = "W0004"; // unreachable match arm (a duplicate pattern already covers it)
pub const W0005: &str = "W0005"; // unreachable code after a return/break/continue
pub const W0006: &str = "W0006"; // unused local binding (`let x = …` never read)
pub const W0007: &str = "W0007"; // `expr // N` — Python floor division silently read as a comment
                                 // Layer-1 ASI warnings
pub const W0701: &str = "W0701"; // uncertainty discarded (Uncertain<T>.value used without checking .confidence)
pub const W1103: &str = "W1103"; // @[verify] outside the SMT-provable fragment (R9); runtime gate applies
                                 // R3c AI-budget warning
pub const W1311: &str = "W1311"; // @[ai(policy(budget: N))] value is not a non-negative integer; ignored
                                 // R6 registry warnings
pub const W1210: &str = "W1210"; // use resolved by AXON_PATH with no lockfile entry (dev mode, unaudited)
                                 // R10 self-improving warnings
pub const W1410: &str = "W1410"; // pass claims `faster` but the perf gate (G4) was not run
                                 // R18 signal quality warnings
pub const W2001: &str = "W2001"; // @[goal] string is vague (no file ref, no measurable criterion, or < 5 words)
                                 // R3 AI-primitive warning
pub const W1310: &str = "W1310"; // live AI call by a fn with no @[ai(policy)] (un-metered/un-pinned)

// Info codes
pub const I0001: &str = "I0001"; // deferred attribute (AI annotations)
pub const I0002: &str = "I0002"; // a foreign keyword was accepted as a no-op (`let mut x`)

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

/// The distance past which [`levenshtein`] stops computing and SATURATES.
///
/// Read this before comparing a `levenshtein` result against anything: the
/// function is bounded, not exact. Two names 10 edits apart come back as 4, so a
/// caller testing `dist <= 4` accepts every string in the program. That is not
/// hypothetical — scaling the suggestion cutoff by name length produced exactly
/// 4 for a 14-character name and turned `suggest("last_rows_data")` from `None`
/// into `Some("rows")`, caught by a unit test that had pinned the old answer.
pub const LEVENSHTEIN_CUTOFF: usize = 3;

/// How far a name may be from a candidate before a "did you mean" suggestion is
/// noise rather than help, scaled to the length of the name the AUTHOR wrote.
///
/// A flat cutoff is meaningless on short names: at `<= 3`, a four-character name
/// matches anything sharing one character, which is how `rows` came to suggest
/// `pow` and `idx` to suggest `lidt`. `max(len, 3) / 3` is rustc's rule.
///
/// Clamped to [`LEVENSHTEIN_CUTOFF`] because the distance function saturates
/// there — an unclamped value of 4 or more does not mean "be more generous", it
/// means "accept everything". The clamp binds only for names of 12 characters or
/// more, where a bounded distance cannot tell near from far anyway.
pub fn suggestion_cutoff(name: &str) -> usize {
    std::cmp::min(
        std::cmp::max(name.chars().count(), 3) / 3,
        LEVENSHTEIN_CUTOFF,
    )
}

/// Classic Wagner-Fischer Levenshtein distance using rolling-row DP.
///
/// BOUNDED: returns `LEVENSHTEIN_CUTOFF + 1` for names further apart than the
/// cutoff, rather than the true distance, to avoid allocating large matrices for
/// clearly-unrelated names. Compare results against [`suggestion_cutoff`], never
/// against a literal.
pub fn levenshtein(a: &str, b: &str) -> usize {
    const CUTOFF: usize = LEVENSHTEIN_CUTOFF;
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

/// Every diagnostic code this compiler defines, with a one-line description.
///
/// THE SOURCE OF TRUTH for what a code means. Before this existed the meanings
/// lived only in `//` comments beside each `pub const`, so no tool could
/// enumerate them: `axon reference` could list 25 verbs and 338 builtins and
/// not one of the ~138 codes a user actually hits.
///
/// A code marked RESERVED is declared but emitted nowhere, and its description
/// says WHERE the condition is actually reported instead — established by
/// probing the real compiler, not by reading the specs that reserved the code.
/// ELEVEN of the thirteen are duplicates of codes that already exist and give
/// better messages (E0803's condition is E0504's "fn `render` requires `T: Show`,
/// but `i64` does not implement `Show`"); TWO belong to unbuilt features
/// (E0912 browser AOT link, E1701/E1703 HAL — three codes, two features). Only
/// E0801 (generic instantiation depth) names a bound nothing enforces, and it
/// would take polymorphic recursion to reach.
///
/// So none should be wired: a reader who meets one in an old log needs a
/// POINTER, not a second implementation of a diagnostic that already exists.
///
/// Listing those as if
/// they were live would be the same defect the reference exists to fix — a
/// document that cannot say "no".
///
/// Gated by `every_error_code_is_in_the_registry`: adding a `pub const` without
/// a row here fails the build.
pub const ALL_CODES: &[(&str, &str)] = &[
    ("E0101", "type inference failed to resolve a type"),
    ("E0102", "type mismatch (arithmetic operands, let annotation, or unification)"),
    ("E0000", "generic parse/IO error with no more-specific code (CLI + LSP)"),
    ("E0001", "cannot find name in this scope"),
    ("E0002", "the name is defined more than once in this module"),
    ("E0003", "module not found on AXON_PATH"),
    ("E0004", "reserved (Phase 2): use of a non-exported item across modules"),
    ("E0301", "type-check failure with no more-specific code"),
    ("E0302", "a `Result` returned by a call is unused — warns by default, error under AXON_STRICT"),
    ("E0303", "type-check rule violation (Phase-1 R03)"),
    ("E0304", "non-exhaustive match — a variant has no arm"),
    ("E0305", "wrong number of arguments supplied to a function"),
    ("E0306", "cannot call a non-function value"),
    ("E0307", "return type mismatch between the declared type and the body"),
    ("E0308", "unknown type named in a signature or annotation"),
    ("E0309", "type-check rule violation (Phase-1 R08)"),
    ("E0310", "RESERVED — the condition is reported as W0006 (unused variable); never emitted under this code"),
    ("E0311", "RESERVED — the condition is reported as W0005 (unreachable code), with a help; never emitted under this code"),
    ("E0312", "RESERVED — the condition is reported as E0304 (non-exhaustive match); never emitted under this code"),
    ("E0313", "RESERVED — the condition is reported as E0306 (cannot call a non-function value); never emitted under this code"),
    ("E0314", "RESERVED — the condition is reported as E0102 (arithmetic on non-numeric type); never emitted under this code"),
    ("E0315", "RESERVED — the condition is reported as E0102 (assignment type mismatch); never emitted under this code"),
    ("E0401", "a field name that does not exist on the thing it is used with — a struct field, an enum-variant pattern field, a tuple index out of range, or a non-numeric tuple index"),
    ("E0402", "an index that cannot be valid — a non-array receiver, `for x in` over one, or a constant index proved out of range"),
    ("E0403", "calling a data field as a method (`p.x()`)"),
    ("E0404", "a path names something that does not exist, or is used as the wrong kind of thing — an unknown variant, an unknown type/module qualifier, or a variant called like a function"),
    ("E0405", "literal pattern's type can't match the match subject"),
    ("E0406", "a field is set more than once in a struct literal"),
    ("E0407", "integer division/remainder by a literal zero"),
    ("E0501", "trait method not implemented"),
    ("E0502", "impl block missing method"),
    ("E0503", "dyn trait cannot be used as value type"),
    ("E0504", "trait bound not satisfied"),
    ("E0601", "use of moved value"),
    ("E0602", "cannot move borrowed value"),
    ("E0603", "borrow conflict"),
    ("E0701", "expression not comptime-evaluable"),
    ("E0702", "comptime integer division by zero"),
    ("E0703", "comptime integer overflow"),
    ("E0800", "LSP: source could not be parsed (document-level diagnostic)"),
    ("E0801", "RESERVED — generic instantiation depth; no bound is enforced today (would need polymorphic recursion to trigger)"),
    ("E0802", "RESERVED — the condition is reported as E0102 (type mismatch in the argument); never emitted under this code"),
    ("E0803", "RESERVED — the condition is reported as E0504 (trait bound not satisfied), with a better message; never emitted under this code"),
    ("E0901", "module not found (AXON_PATH search failed)"),
    ("E0902", "circular import between source files"),
    ("E0903", "duplicate top-level name across files"),
    ("E0904", "--target triple not supported by this LLVM build"),
    ("E0905", "cross-compilation needs sysroot (cross.toml missing)"),
    ("E0906", "cache entry corrupt or wrong compiler version"),
    ("E0907", "AOT wasm build needs the native codegen backend (R7)"),
    ("E0908", "RESERVED — the condition is reported as E0904 (target not supported by this LLVM build); never emitted under this code"),
    ("E0910", "builtin / construct has no native codegen lowering — honest abort, runs under the interpreter"),
    ("E0911", "browser target (--host browser): a browser-incompatible builtin can't run in the tab — clean refusal mirroring E0910 (R7c)"),
    ("E0912", "RESERVED — browser AOT link failure (R7c); the feature is unbuilt, so the condition cannot arise"),
    ("W0913", "sleep_ms is a no-op on the browser host (main thread can't block) (R7c)"),
    ("E1001", "I/O call not permitted by @[contained] spec"),
    ("E1002", "@[contained] clause is malformed"),
    ("E1003", "capability path is not parseable"),
    ("E1004", "call hits a never: clause (hard violation)"),
    ("E1005", "R45: an un-granted capability is reached from an entry point under `--require-contained`; distinct from E1001, which means a grant EXISTS and was exceeded"),
    ("E1101", "verify bound not satisfied (runtime gate)"),
    ("E1102", "verify bound statically VIOLABLE — SMT counterexample (R9)"),
    ("E1201", "on-disk module bytes ≠ axon.lock hash (tamper)"),
    ("E1202", "use with no lockfile entry under --locked"),
    ("E1203", "import declares capabilities beyond the importer's grant"),
    ("E1204", "lockfile audit verdict is `denied`"),
    ("E1205", "axon.lock is malformed / unknown version"),
    ("E1206", "a @[sensitive] value flows into an external AI call (PRD §4 privacy)"),
    ("E1207", "a @[pure] function performs/contains an impure operation (Phase 5 §2, P01/P02)"),
    ("E1208", "a @[total] function has no strictly-decreasing measure at a recursive call (Phase 5 §3)"),
    ("E1209", "a constant argument provably violates a refinement-type predicate (Phase 5 §1 R02)"),
    ("E1210", "a `sql_query` template is not a string literal — user data must be a bound parameter, never concatenated SQL (injection)"),
    ("E1300", "ai_* call unreachable and no @[ai(policy(fallback))] in scope"),
    ("E1301", "ai_complete exceeded the fn's @[ai(policy(budget: N))] (R3c)"),
    ("E1302", "tier: resolves to a tier with no host-configured model"),
    ("E1303", "run exceeded AXON_BUDGET_TOKENS, the ambient run-level token cap"),
    ("E1306", "raw effect-row syntax `| {…}` used in a `surface`-marked file"),
    ("E1310", "effect-row leak: call performs effect E ∉ the caller's declared row"),
    ("E1316", "@[contained(...)] deprecation notice; prefer `| {…}` effect-row syntax"),
    ("E1610", "a kernel capability-mint obligation (attenuation / budget-carve) is not SMT-discharged — the minter has been weakened (I-12 tripwire)"),
    ("E1611", "TCB attestation mismatch at boot: the kernel obligation digest ≠ the pinned manifest (the proven TCB changed without a manifest update — I-12)"),
    ("E1401", "G1 correctness: pass changes observable output on a corpus member"),
    ("E1402", "G2 safety: pass adds a capability the original lacked (I-12)"),
    ("E1403", "G3 regression: pass breaks an existing test"),
    ("E1404", "graduation requires multi-sig of root Principals (I-12)"),
    ("E1405", "pass manifest hash mismatch at boot (TCB attestation)"),
    ("E1406", "correctness judged by AI — forbidden; the oracle is the interpreter"),
    ("E1407", "AI proposed a template name not in the closed registry (rejected before verify)"),
    ("E1408", "a graduated/verified pass name is absent from the template registry (tamper / version skew)"),
    ("E1409", "RewriteSpec empty / not provably total (proposes no transform)"),
    ("E1411", "RewriteSpec rule name outside the closed reviewed vocabulary"),
    ("E1412", "RewriteSpec could express a capability (grammar violation — defense in design)"),
    ("E1413", "RewriteSpec over the rule-count budget (runaway proposal rejected, never run)"),
    ("E1500", "metric must name an @[adaptive] fn"),
    ("E1503", "target/max_evals/holdout must parse as numbers"),
    ("E1504", "#[goal] fn must have zero params"),
    ("E1505", "unknown #[goal(strategy: …)] — not hill_climb|random|multistart|tournament|bayesian"),
    ("E1900", "R19: integer literal out of range for its fixed-width/unsigned annotation"),
    ("E2200", "str_slice byte range splits a UTF-8 character (runtime panic text)"),
    ("E2201", "malformed JSON where a document was required (Err-string prefix)"),
    ("E2202", "JSON type mismatch at a path/element (Err-string prefix)"),
    ("E2203", "regex construct requiring backtracking, or a pattern too large once counted repetitions expand"),
    ("E2204", "base64/hex decode: invalid input, or bytes that are not valid UTF-8"),
    ("E2205", "re_replace_all: replacement references a capture group the pattern does not have"),
    ("E1700", "raw pointer `*T`, volatile_*, ptr_from_addr, or @[hal] used in a `surface` file"),
    ("E1701", "RESERVED — @[hal] capability check (R17); the feature is unbuilt, so the condition cannot arise"),
    ("E1702", "freestanding build has no @[entry] or @[panic_handler]"),
    ("E1703", "RESERVED — surface caller reaching a Hal-effected fn (R17); the feature is unbuilt, so the condition cannot arise"),
    ("E1704", "@[no_alloc] fn reaches a heap-allocating builtin (ISR/early-boot alloc-free guarantee)"),
    ("E1706", "R17 Slice 2: atomic ordering arg is not a compile-time literal in 0..=4"),
    ("E1707", "R17 §12 Q7: fn_addr's argument is not a compile-time string literal, or names no known function"),
    ("E1710", "--host mobile but the mobile toolchain is absent: the Android NDK (linker on PATH) for an *-android triple, or Xcode `xcrun` for an *-apple-ios triple"),
    ("E1711", "axon-rt / a native module not cross-built for the device triple (iOS-specific staging code)"),
    ("E1712", "mobile cross-link failed (iOS xcframework or Android jniLibs packaging)"),
    ("E1800", "`use native::M` for an unregistered module name"),
    ("E1801", "native call arg/ret type not FFI-representable"),
    ("E1802", "handle of module A passed where module B's handle expected"),
    ("E1803", "arithmetic / forging on a `Handle` (opaque, unconstructable)"),
    ("E1810", "`tee_unseal` (Secret declassification) called outside an `@[enclave]` fn"),
    ("E2300", "a BPF helper not on the Axon capability allowlist is called from a @[bpf] program"),
    ("E2301", "a construct outside the BPF-lowerable subset appears in a @[bpf] body"),
    ("E2302", "@[bpf(kind: K)] has an unknown program kind"),
    ("E2400", "redefining a name in a session breaks an item an earlier cell wrote"),
    ("E2402", "a session cell is not a usable fragment — today this means it declares its own `fn main`, which collides with the one the session composes around the cell's statements"),
    ("E2403", "malformed `axon session --protocol jsonl` input frame"),
    ("W0001", "unknown attribute"),
    ("W0002", "variable shadowing"),
    ("W0003", "user fn shadows a builtin (builtin takes precedence)"),
    ("W0004", "unreachable match arm (a duplicate pattern already covers it)"),
    ("W0005", "unreachable code after a return/break/continue"),
    ("W0006", "unused local binding (`let x = …` never read)"),
    ("W0007", "`expr // N` — Python floor division silently read as a comment"),
    ("W0701", "uncertainty discarded (Uncertain<T>.value used without checking .confidence)"),
    ("W1103", "@[verify] outside the SMT-provable fragment (R9); runtime gate applies"),
    ("W1311", "@[ai(policy(budget: N))] value is not a non-negative integer; ignored"),
    ("W1210", "use resolved by AXON_PATH with no lockfile entry (dev mode, unaudited)"),
    ("W1410", "pass claims `faster` but the perf gate (G4) was not run"),
    ("W2001", "@[goal] string is vague (no file ref, no measurable criterion, or < 5 words)"),
    ("W1310", "live AI call by a fn with no @[ai(policy)] (un-metered/un-pinned)"),
    ("I0001", "deferred attribute (AI annotations)"),
    ("I0002", "a foreign keyword was accepted as a no-op (`let mut x`)"),
];

#[cfg(test)]
mod registry_tests {
    use super::ALL_CODES;

    /// Every `pub const` code must have a row in [`ALL_CODES`].
    ///
    /// This is the OMISSION direction, and it is the one that matters. The
    /// pre-existing duplicate check verified that codes listed in a test array
    /// were distinct — it could not notice a code that was never listed. That
    /// asymmetry is exactly how 11 CLI verbs went undocumented while a gate
    /// reported PASS.
    #[test]
    fn every_error_code_is_in_the_registry() {
        let src = include_str!("error.rs");
        let mut declared: Vec<&str> = Vec::new();
        for line in src.lines() {
            let t = line.trim_start();
            if let Some(rest) = t.strip_prefix("pub const ") {
                if let Some(name) = rest.split(':').next() {
                    let n = name.trim();
                    let is_code = n.len() >= 5
                        && matches!(n.as_bytes()[0], b'E' | b'W' | b'I')
                        && n[1..].chars().all(|c| c.is_ascii_digit());
                    if is_code {
                        declared.push(n);
                    }
                }
            }
        }
        // Guard the guard: a scan that found nothing would satisfy the
        // assertion below vacuously.
        assert!(
            declared.len() > 100,
            "expected to find many codes, found {}",
            declared.len()
        );
        let registered: std::collections::HashSet<&str> =
            ALL_CODES.iter().map(|(c, _)| *c).collect();
        let missing: Vec<&str> = declared
            .iter()
            .copied()
            .filter(|c| !registered.contains(c))
            .collect();
        assert!(
            missing.is_empty(),
            "these codes have no ALL_CODES row: {missing:?}\n\
             Add `(\"CODE\", \"one-line description\"),` to ALL_CODES."
        );
    }

    #[test]
    fn the_registry_has_no_duplicates_and_no_empty_descriptions() {
        let mut seen = std::collections::HashSet::new();
        for (code, desc) in ALL_CODES {
            assert!(seen.insert(*code), "duplicate registry row: {code}");
            assert!(
                !desc.trim().is_empty(),
                "{code} has an empty description — a code with no meaning is \
                 worse than an undocumented one, because it looks documented"
            );
        }
    }

    /// A code the registry calls live must actually be emitted somewhere.
    ///
    /// Thirteen codes were declared and referenced nowhere. A reference that
    /// listed them as supported would be claiming a diagnostic the compiler
    /// cannot produce — the same defect as a report that cannot say "no".
    #[test]
    fn a_code_not_marked_reserved_is_actually_emitted() {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/src");
        let mut corpus = String::new();
        fn walk(dir: &std::path::Path, out: &mut String) {
            let Ok(rd) = std::fs::read_dir(dir) else {
                return;
            };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, out);
                } else if p.extension().is_some_and(|x| x == "rs")
                    && p.file_name().is_some_and(|f| f != "error.rs")
                {
                    if let Ok(t) = std::fs::read_to_string(&p) {
                        out.push_str(&t);
                    }
                }
            }
        }
        walk(std::path::Path::new(root), &mut corpus);
        assert!(
            corpus.len() > 100_000,
            "corpus scan failed — guard is vacuous"
        );

        let unemitted: Vec<&str> = ALL_CODES
            .iter()
            .filter(|(_, d)| !d.contains("RESERVED"))
            .map(|(c, _)| *c)
            .filter(|c| !corpus.contains(*c))
            .collect();
        assert!(
            unemitted.is_empty(),
            "these codes are not marked RESERVED but are emitted nowhere: {unemitted:?}\n\
             Either emit them, or append ` — RESERVED, not currently emitted` to \
             their description."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{levenshtein, suggestion_cutoff, LEVENSHTEIN_CUTOFF};

    /// `levenshtein` SATURATES. Pinning that is the point: a caller who compares
    /// its result against a number larger than the cutoff accepts everything,
    /// and the failure is silent because the function still returns a plausible
    /// small integer.
    #[test]
    fn levenshtein_saturates_rather_than_returning_a_true_distance() {
        // Ten edits apart, reported as four.
        assert_eq!(
            levenshtein("last_rows_data", "rows"),
            LEVENSHTEIN_CUTOFF + 1
        );
        assert_eq!(levenshtein("a", "abcdefghijklmnop"), LEVENSHTEIN_CUTOFF + 1);
        // Below the cutoff it is exact.
        assert_eq!(levenshtein("prntln", "println"), 1);
        assert_eq!(levenshtein("println", "println"), 0);
    }

    /// Which is why the suggestion cutoff can never reach the saturation value:
    /// at `LEVENSHTEIN_CUTOFF + 1` every pair of names looks similar.
    #[test]
    fn the_suggestion_cutoff_never_reaches_the_saturation_value() {
        for name in [
            "a",
            "xy",
            "rows",
            "calculate",
            "last_rows_data",
            &"z".repeat(64),
        ] {
            assert!(
                suggestion_cutoff(name) <= LEVENSHTEIN_CUTOFF,
                "cutoff for `{name}` must stay under saturation"
            );
        }
        // Scaled, not flat: a short name gets a tight cutoff.
        assert_eq!(suggestion_cutoff("rows"), 1);
        assert_eq!(suggestion_cutoff("calculate"), 3);
    }

    use super::*;

    #[test]
    fn error_codes_are_unique() {
        let codes = [
            E0000, E0001, E0002, E0003, E0004, E0101, E0102, E0301, E0302, E0303, E0304, E0305,
            E0306, E0307, E0308, E0309, E0310, E0311, E0312, E0313, E0314, E0315, E0401, E0402,
            E0403, E0404, E0405, E0406, E0407, E0501, E0502, E0503, E0504, E0601, E0602, E0603,
            E0701, E0702, E0703, E0800, E0801, E0802, E0803, E0901, E0902, E0903, E0904, E0905,
            E0906, E0907, E0908, E0910, E0911, E0912, E1001, E1002, E1003, E1004, E1101, E1102,
            E1201, E1202, E1203, E1204, E1205, E1206, E1207, E1208, E1209, E1300, E1301, E1302,
            E1303, E1306, E1310, E1316, E1401, E1402, E1403, E1404, E1405, E1406, E1407, E1408,
            E1409, E1411, E1412, E1413, E1500, E1503, E1504, E1505, E1700, E1701, E1702, E1703,
            E1704, E1706, E1707, E1710, E1711, E1712, E1800, E1801, E1802, E1803, E1810, E1900,
            E2300, E2301, E2302, E2400, E2402, E2403, E2200, E2201, E2202, E2203, E2204, E2205,
            W0001, W0002, W0003, W0004, W0005, W0006, W0007, W0701, W0913, W1103, W1210, W1310,
            W1311, W1410, W2001, I0001,
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
        let e = AxonError::new(E0001, "undefined variable").at("main.ax", 5, 10);
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
        let e = AxonError::new(E0001, "undefined").fix("did you mean 'foo'?");
        let d = e.display();
        assert!(d.contains("fix:"));
        assert!(d.contains("foo"));
    }
}
