//! Self-improving-compiler verification harness (R10).
//!
//! R10 is the single highest-risk component in the PRD: a compiler that can
//! modify itself can, if unverified, silently miscompile every downstream
//! program or grant itself capabilities. The spec's resolution is to **specify
//! and build the verification harness before any discovery machinery exists**,
//! so an AI-discovered "optimization" cannot graduate into a compiler pass
//! without clearing a gate that is itself part of the TCB.
//!
//! A candidate **pass** is a pure transform on the AST (`Program -> Program`).
//! [`verify_pass`] runs it through gates in order; any failure rejects it:
//!
//! - **G1 Correctness** (the oracle): for every program `c` in the corpus,
//!   `interp(P(c))` must produce the *same observable tuple* — `(exit_code,
//!   stdout)` — as `interp(c)`. The equivalence oracle is the **interpreter**
//!   (I-2), never an AI judgment (that is forbidden — see [`VerifyError`]
//!   `E1406`). A pass that changes output for *any* member fails (`E1401`):
//!   there is no "close enough". This is what catches the headline attack — a
//!   pass that is faster *because* it drops a bounds check.
//! - **G2 Safety** (I-12): the transformed program may not exercise a
//!   capability the original lacked — `caps(P(c)) ⊆ caps(c)`. A pass that adds
//!   `read_file`/`write_file`/`net`/`exec` is rejected (`E1402`). This is the
//!   firewall that stops a self-modifying compiler from widening the trusted
//!   surface.
//!
//! G3 (regression suite) and G4 (native perf timing) are documented-pending in
//! this slice (their harnesses are separate); [`VerifyRecord`] carries their
//! status honestly rather than claiming a green they didn't earn.
//!
//! Determinism (R10 §4.7): G1 compares observable tuples under the existing
//! seed/mock controls (`AXON_SEED`, `AXON_AI_MOCK`), so two `verify` runs of the
//! same `(pass, corpus)` produce identical records. A non-deterministic verifier
//! of a self-modifying compiler would itself be a vulnerability.

use crate::ast::Program;
use crate::capabilities::program_capabilities;
use crate::error::{E1401, E1402};
use crate::interp::run_program_capturing;

/// A candidate compiler pass: a pure AST→AST transform. The harness treats it
/// as opaque — it only observes the *behavior* of the output, never trusts the
/// pass to describe itself.
pub type Pass = dyn Fn(&Program) -> Program;

/// Status of a verification gate that this slice does not yet run, recorded
/// honestly so a `VerifyRecord` never implies a green it didn't earn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateStatus {
    Passed,
    /// Gate not run in this build (G3 suite / G4 timing harness pending).
    NotRun,
}

/// Why a pass was rejected. Carries the stable diagnostic code (R10 §6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyError {
    pub code: &'static str,
    pub message: String,
}

/// The immutable outcome of verifying a pass against a corpus (R10 §4.4).
/// `Ok` means G1+G2 held over the whole corpus; `Err` names the first gate
/// failure with its code. Deterministic for a fixed `(pass, corpus)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyRecord {
    /// Number of corpus members checked.
    pub members: usize,
    /// G1 correctness verdict (interpreter oracle over the whole corpus).
    pub g1_correctness: Result<(), VerifyError>,
    /// G2 capability-safety verdict (caps(P(c)) ⊆ caps(c) for every member).
    pub g2_safety: Result<(), VerifyError>,
    /// G3 regression suite — pending in this slice.
    pub g3_regression: GateStatus,
    /// G4 native perf timing — pending in this slice.
    pub g4_perf: GateStatus,
}

impl VerifyRecord {
    /// The pass is eligible for graduation *only* if every gate that ran
    /// passed. (Graduation itself additionally requires multi-sig — that is a
    /// separate, human-gated step; passing verification is necessary, not
    /// sufficient. R10 §4.5.)
    pub fn passed(&self) -> bool {
        self.g1_correctness.is_ok() && self.g2_safety.is_ok()
    }

    /// The first gate failure, if any.
    pub fn rejection(&self) -> Option<&VerifyError> {
        self.g1_correctness.as_ref().err().or(self.g2_safety.as_ref().err())
    }
}

/// Verify a candidate pass against a corpus of programs.
///
/// Runs G1 (correctness, the interpreter oracle) and G2 (capability safety)
/// over every corpus member. Stops G1/G2 at the first violating member and
/// records its code — but always reports which member and why, so the rejection
/// is auditable. The corpus is iterated in full (never "just the discovering
/// program") — verifying against a single program is the exact overfitting the
/// harness exists to prevent (R10 §4.2).
///
/// Note on the oracle (R10 §7, E1406): correctness here is decided *solely* by
/// running both programs through the interpreter and comparing observable
/// output. There is deliberately no path by which an AI judges correctness — a
/// self-improving compiler that trusts an AI to say "this is still correct"
/// would have no firewall at all.
pub fn verify_pass(pass: &Pass, corpus: &[Program]) -> VerifyRecord {
    let mut g1: Result<(), VerifyError> = Ok(());
    let mut g2: Result<(), VerifyError> = Ok(());

    for (i, original) in corpus.iter().enumerate() {
        let transformed = pass(original);

        // G2 first on the AST (cheap, and a capability widening is the most
        // dangerous failure — flag it before executing the transformed code).
        let caps_before = program_capabilities(original);
        let caps_after = program_capabilities(&transformed);
        let added: Vec<String> = caps_after.difference(&caps_before).cloned().collect();
        if !added.is_empty() && g2.is_ok() {
            g2 = Err(VerifyError {
                code: E1402,
                message: format!(
                    "G2 safety: pass adds capabilit{} {{{}}} not present in corpus member #{i} \
                     — a pass may never widen the capability surface (I-12)",
                    if added.len() == 1 { "y" } else { "ies" },
                    added.join(", ")
                ),
            });
        }

        // G1: observable equivalence via the interpreter oracle.
        if g1.is_ok() {
            let before = run_program_capturing(original);
            let after = run_program_capturing(&transformed);
            if before != after {
                let (bc, bo) = &before;
                let (ac, ao) = &after;
                let detail = if bc != ac {
                    format!("exit code {bc} → {ac}")
                } else {
                    format!("stdout changed ({} → {} bytes)", bo.len(), ao.len())
                };
                g1 = Err(VerifyError {
                    code: E1401,
                    message: format!(
                        "G1 correctness: pass changes observable output on corpus member #{i} \
                         ({detail}) — the transformed program must be behaviorally identical"
                    ),
                });
            }
        }

        // Both gates have a verdict (pass or first failure); keep iterating only
        // to confirm there isn't an *earlier*-indexed issue is unnecessary — we
        // report the first. Break once both have failed (nothing more to learn).
        if g1.is_err() && g2.is_err() {
            break;
        }
    }

    VerifyRecord {
        members: corpus.len(),
        g1_correctness: g1,
        g2_safety: g2,
        g3_regression: GateStatus::NotRun,
        g4_perf: GateStatus::NotRun,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_source;

    fn prog(src: &str) -> Program {
        parse_source(src).expect("parse corpus member")
    }

    fn corpus() -> Vec<Program> {
        vec![
            prog("fn main() -> i64 { 21 + 21 }"),
            prog("fn main() -> i64 { let x = 5  x * 2 }"),
            prog("fn main() { println(\"hello\") }"),
        ]
    }

    /// The headline red test (R10 §8): an identity pass verifies; a pass that
    /// changes observable output is rejected with E1401.
    #[test]
    fn identity_pass_verifies_and_output_changing_pass_is_rejected() {
        let c = corpus();

        // Identity: P(c) == c. Must pass G1 and G2.
        let identity: &Pass = &|p: &Program| p.clone();
        let rec = verify_pass(identity, &c);
        assert!(rec.passed(), "identity pass must verify: {:?}", rec.rejection());
        assert_eq!(rec.members, 3);

        // Output-changing: rewrite every `main` to return 0. Changes the exit
        // code of the arithmetic programs → G1 fail (E1401).
        let breaker: &Pass = &|p: &Program| {
            use crate::ast::{Expr, Item, Literal};
            let mut np = p.clone();
            for item in &mut np.items {
                if let Item::FnDef(f) = item {
                    if f.name == "main" {
                        f.body = Expr::Literal(Literal::Int(0));
                    }
                }
            }
            np
        };
        let rec = verify_pass(breaker, &c);
        assert!(!rec.passed(), "an output-changing pass must be rejected");
        let err = rec.rejection().expect("a rejection");
        assert_eq!(err.code, E1401, "must be G1 correctness failure: {}", err.message);
    }

    /// G2 safety core (R10 §8): a pass that adds a capability the original
    /// lacked is rejected with E1402 (I-12), even if output were unchanged.
    #[test]
    fn capability_adding_pass_is_rejected_i12() {
        // Corpus member does NO I/O.
        let c = vec![prog("fn main() -> i64 { 1 + 1 }")];

        // Malicious pass: inject a read_file call into main's body.
        let exfil: &Pass = &|p: &Program| {
            use crate::ast::{Expr, Item, Literal, Stmt};
            let mut np = p.clone();
            for item in &mut np.items {
                if let Item::FnDef(f) = item {
                    if f.name == "main" {
                        // { let _ = read_file("/etc/passwd")  <orig body> }
                        let read = Expr::Call {
                            callee: Box::new(Expr::Ident("read_file".into())),
                            args: vec![Expr::Literal(Literal::Str("/etc/passwd".into()))],
                        };
                        let orig = f.body.clone();
                        f.body = Expr::Block(vec![
                            Stmt { expr: read, span: crate::span::Span::dummy() },
                            Stmt { expr: orig, span: crate::span::Span::dummy() },
                        ]);
                    }
                }
            }
            np
        };
        let rec = verify_pass(exfil, &c);
        assert!(!rec.passed(), "a capability-adding pass must be rejected");
        let err = rec.rejection().expect("a rejection");
        assert_eq!(err.code, E1402, "must be G2 safety failure: {}", err.message);
        assert!(err.message.contains("fs:read"), "names the added capability: {}", err.message);
    }

    /// Determinism (R10 §4.7): two verify runs of the same (pass, corpus)
    /// produce identical records.
    #[test]
    fn verify_record_is_deterministic() {
        let c = corpus();
        let identity: &Pass = &|p: &Program| p.clone();
        let a = verify_pass(identity, &c);
        let b = verify_pass(identity, &c);
        assert_eq!(a, b, "verify must be deterministic for a fixed (pass, corpus)");
    }

    /// An overfit pass — correct on one member, wrong on another — is still
    /// caught, because G1 iterates the WHOLE corpus, not the discovering
    /// program (R10 §4.2, the fork's central concern).
    #[test]
    fn overfit_pass_passing_on_one_member_is_caught() {
        let c = corpus(); // members 0,1 return ints; member 2 prints.
        // A pass that only rewrites a program returning exactly 42 to return 42
        // (a no-op on member 0) but corrupts any *other* int-returning main to 0.
        let overfit: &Pass = &|p: &Program| {
            use crate::ast::{Expr, Item, Literal};
            let (code, _) = run_program_capturing(p);
            let mut np = p.clone();
            if code != 42 {
                for item in &mut np.items {
                    if let Item::FnDef(f) = item {
                        if f.name == "main" {
                            f.body = Expr::Literal(Literal::Int(0));
                        }
                    }
                }
            }
            np
        };
        let rec = verify_pass(overfit, &c);
        assert!(!rec.passed(), "overfit pass (correct on member 0, wrong on 1) must be caught");
        assert_eq!(rec.rejection().unwrap().code, E1401);
    }
}
