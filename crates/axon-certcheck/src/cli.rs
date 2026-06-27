//! R23 §5.2 — the `certcheck` command surface: prove / check / explain. Thin I/O
//! shell over the pure core. Output is human-legible (plain English about the
//! trust shift), not just exit codes; every subcommand has `--help`.
//!
//! Exit codes (§3.4): 0 Valid · 2 malformed/usage · 4 Unsupported · 8 Invalid.

use crate::certificate::{ProofCertificate, Refutation};
use crate::checker::{check, CheckResult};
use crate::obligation::Obligation;
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "\
certcheck — validate Axon proof certificates with NO SMT solver in the trust path

USAGE:
    certcheck check   <obligation.obl> <certificate.cert>   [SOLVER-FREE — the trusted op]
    certcheck explain <obligation.obl> [--cert certificate.cert]
    certcheck prove   <obligation.obl> [--out cert.cert]    [requires the `smt` build]

Every subcommand accepts --help. Exit codes: 0 valid, 2 usage/malformed,
4 unsupported (outside the certified fragment), 8 invalid (cert does not prove it).";

/// Entry point. Returns the process exit code.
pub fn run(args: Vec<String>) -> ExitCode {
    let a: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();
    match a.as_slice() {
        [] | ["--help"] | ["-h"] | ["help"] => {
            println!("{USAGE}");
            ExitCode::from(0)
        }
        ["check", "--help"] => help(
            "check <obligation.obl> <certificate.cert>  — independently validate the certificate \
             WITHOUT a solver",
        ),
        ["explain", "--help"] => help(
            "explain <obligation.obl> [--cert certificate.cert]  — pretty-print the claim + a \
             readable walk of the refutation",
        ),
        ["prove", "--help"] => help(
            "prove <obligation.obl> [--out cert.cert]  — find a proof via the oracle and emit a \
             certificate (requires the `smt` build)",
        ),
        ["check", obl, cert] => cmd_check(Path::new(obl), Path::new(cert)),
        ["explain", rest @ ..] => cmd_explain(rest),
        ["prove", rest @ ..] => cmd_prove(rest),
        _ => {
            eprintln!("certcheck: unrecognized invocation\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn help(line: &str) -> ExitCode {
    println!("certcheck {line}");
    ExitCode::from(0)
}

fn read(path: &Path) -> Result<String, ExitCode> {
    std::fs::read_to_string(path).map_err(|e| {
        eprintln!("certcheck: cannot read {}: {e}", path.display());
        ExitCode::from(2)
    })
}

fn load_obligation(path: &Path) -> Result<Obligation, ExitCode> {
    let s = read(path)?;
    Obligation::parse(&s).map_err(|e| {
        eprintln!("certcheck: bad obligation {}: {}", path.display(), e);
        ExitCode::from(2)
    })
}

fn load_certificate(path: &Path) -> Result<ProofCertificate, ExitCode> {
    let s = read(path)?;
    ProofCertificate::parse(&s).map_err(|e| {
        eprintln!("certcheck: bad certificate {}: {}", path.display(), e);
        ExitCode::from(2)
    })
}

/// `certcheck check` — the trusted op. Solver-free validation.
fn cmd_check(obl_path: &Path, cert_path: &Path) -> ExitCode {
    let obl = match load_obligation(obl_path) {
        Ok(o) => o,
        Err(c) => return c,
    };
    let cert = match load_certificate(cert_path) {
        Ok(c) => c,
        Err(c) => return c,
    };
    match check(&obl, &cert) {
        CheckResult::Valid => {
            println!(
                "\u{2713} VALID \u{2014} the obligation `{}` holds for all inputs; verified WITHOUT a \
                 solver (checker only).",
                obl.id
            );
            ExitCode::from(0)
        }
        CheckResult::Invalid { reason } => {
            println!("\u{2717} INVALID: {reason}");
            ExitCode::from(8)
        }
        CheckResult::Unsupported { reason } => {
            println!("\u{2022} UNSUPPORTED: {reason} (outside the certified fragment)");
            ExitCode::from(4)
        }
    }
}

/// `certcheck explain` — pretty-print the claim and (optionally) the refutation.
fn cmd_explain(rest: &[&str]) -> ExitCode {
    let mut obl: Option<&str> = None;
    let mut cert: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--cert" if i + 1 < rest.len() => {
                cert = Some(rest[i + 1]);
                i += 2;
            }
            s if !s.starts_with("--") && obl.is_none() => {
                obl = Some(s);
                i += 1;
            }
            _ => {
                eprintln!("certcheck explain: bad argument `{}`", rest[i]);
                return ExitCode::from(2);
            }
        }
    }
    let Some(obl) = obl else {
        eprintln!("certcheck explain: missing <obligation.obl>");
        return ExitCode::from(2);
    };
    let obligation = match load_obligation(Path::new(obl)) {
        Ok(o) => o,
        Err(c) => return c,
    };
    println!("Obligation: {}", obligation.id);
    println!(
        "  Variables: {}",
        obligation
            .vars
            .iter()
            .map(|v| format!("{}: {:?}", v.name, v.sort))
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!("  Claim (holds for ALL inputs):");
    println!("    {}", crate::cli::fmt_formula(&obligation.claim));
    if let Some(cert) = cert {
        let c = match load_certificate(Path::new(cert)) {
            Ok(c) => c,
            Err(code) => return code,
        };
        println!("  Refutation of the negated claim (the proof the checker confirmed):");
        explain_refutation(&c.refutation, 2);
    }
    ExitCode::from(0)
}

fn explain_refutation(r: &Refutation, depth: usize) {
    let pad = "  ".repeat(depth);
    match r {
        Refutation::BoolCase {
            var,
            on_true,
            on_false,
        } => {
            println!("{pad}split on boolean {var}:");
            println!("{pad}  case {var}=\u{22a4}:");
            explain_refutation(on_true, depth + 2);
            println!("{pad}  case {var}=\u{22a5}:");
            explain_refutation(on_false, depth + 2);
        }
        Refutation::IteCase {
            cond,
            on_true,
            on_false,
        } => {
            println!("{pad}split on condition {}:", fmt_formula(cond));
            println!("{pad}  case \u{22a4}:");
            explain_refutation(on_true, depth + 2);
            println!("{pad}  case \u{22a5}:");
            explain_refutation(on_false, depth + 2);
        }
        Refutation::LinearContradiction { facts, coeffs } => {
            let combo: Vec<String> = facts
                .iter()
                .zip(coeffs)
                .map(|(f, c)| format!("{c}\u{b7}({})", fmt_fact(f)))
                .collect();
            println!(
                "{pad}Farkas: {} \u{21d2} 0 < 0 (contradiction)",
                combo.join(" + ")
            );
        }
    }
}

fn fmt_fact(f: &crate::certificate::LinFact) -> String {
    let terms: Vec<String> = f
        .terms
        .iter()
        .map(|(c, v)| format!("{c}\u{b7}{v}"))
        .collect();
    let lhs = if terms.is_empty() {
        "0".to_string()
    } else {
        terms.join(" + ")
    };
    let op = match f.op {
        crate::certificate::FactOp::Le => "\u{2264}",
        crate::certificate::FactOp::Lt => "<",
    };
    format!("{lhs} {op} {}", f.constant)
}

/// A readable rendering of a formula (used by `explain`).
pub fn fmt_formula(f: &crate::obligation::Formula) -> String {
    use crate::obligation::Formula as F;
    match f {
        F::Bool { v } => v.to_string(),
        F::BVar { name } => name.clone(),
        F::Not { f } => format!("\u{ac}({})", fmt_formula(f)),
        F::And { fs } => fs
            .iter()
            .map(fmt_formula)
            .collect::<Vec<_>>()
            .join(" \u{2227} "),
        F::Or { fs } => fs
            .iter()
            .map(fmt_formula)
            .collect::<Vec<_>>()
            .join(" \u{2228} "),
        F::Implies { a, b } => format!("({} \u{21d2} {})", fmt_formula(a), fmt_formula(b)),
        F::Cmp { op, l, r } => format!("{} {} {}", fmt_term(l), fmt_op(*op), fmt_term(r)),
    }
}

fn fmt_op(op: crate::obligation::Op) -> &'static str {
    use crate::obligation::Op::*;
    match op {
        Lt => "<",
        Le => "\u{2264}",
        Gt => ">",
        Ge => "\u{2265}",
        Eq => "=",
        Ne => "\u{2260}",
    }
}

fn fmt_term(t: &crate::obligation::Term) -> String {
    use crate::obligation::Term as T;
    match t {
        T::Int { v } => v.to_string(),
        T::IVar { name } => name.clone(),
        T::Add { l, r } => format!("({} + {})", fmt_term(l), fmt_term(r)),
        T::Sub { l, r } => format!("({} - {})", fmt_term(l), fmt_term(r)),
        T::MulConst { k, t } => format!("{k}\u{b7}{}", fmt_term(t)),
        T::Min { l, r } => format!("min({}, {})", fmt_term(l), fmt_term(r)),
        T::Max { l, r } => format!("max({}, {})", fmt_term(l), fmt_term(r)),
        T::Abs { t } => format!("abs({})", fmt_term(t)),
        T::Ite { cond, then, els } => format!(
            "ite({}, {}, {})",
            fmt_formula(cond),
            fmt_term(then),
            fmt_term(els)
        ),
    }
}

// ── prove (behind smt) ───────────────────────────────────────────────────────

#[cfg(feature = "smt")]
fn cmd_prove(rest: &[&str]) -> ExitCode {
    let mut obl: Option<&str> = None;
    let mut out: Option<&str> = None;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--out" if i + 1 < rest.len() => {
                out = Some(rest[i + 1]);
                i += 2;
            }
            s if !s.starts_with("--") && obl.is_none() => {
                obl = Some(s);
                i += 1;
            }
            _ => {
                eprintln!("certcheck prove: bad argument `{}`", rest[i]);
                return ExitCode::from(2);
            }
        }
    }
    let Some(obl) = obl else {
        eprintln!("certcheck prove: missing <obligation.obl>");
        return ExitCode::from(2);
    };
    let obligation = match load_obligation(Path::new(obl)) {
        Ok(o) => o,
        Err(c) => return c,
    };
    match crate::emit::prove(&obligation) {
        Ok(cert) => {
            let js = cert.to_json();
            if let Some(out) = out {
                if let Err(e) = std::fs::write(out, &js) {
                    eprintln!("certcheck prove: cannot write {out}: {e}");
                    return ExitCode::from(2);
                }
            } else {
                println!("{js}");
            }
            println!("\u{2713} certified {}", obligation.id);
            ExitCode::from(0)
        }
        Err(crate::emit::ProveErr::Unsupported { reason }) => {
            println!("\u{2022} could not prove (Unsupported): {reason}");
            ExitCode::from(4)
        }
        Err(crate::emit::ProveErr::Counterexample { reason }) => {
            println!("\u{2717} could not prove (SAT counterexample): {reason}");
            ExitCode::from(8)
        }
        Err(crate::emit::ProveErr::Internal { reason }) => {
            eprintln!("certcheck prove: internal error: {reason}");
            ExitCode::from(2)
        }
    }
}

#[cfg(not(feature = "smt"))]
fn cmd_prove(_rest: &[&str]) -> ExitCode {
    eprintln!(
        "certcheck prove: this binary was built WITHOUT the `smt` feature, so it cannot GENERATE \
         certificates (the solver-free build only validates them). Rebuild with \
         `--features smt` to use `prove`, or run `certcheck check` on an existing certificate."
    );
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obligation::build::*;
    use crate::obligation::{Op, Sort, Var};

    #[test]
    fn fmt_formula_is_readable() {
        let f = cmp(Op::Le, min(ivar("g"), ivar("avail")), ivar("avail"));
        let s = fmt_formula(&f);
        assert!(s.contains("min(g, avail)"), "{s}");
    }

    #[test]
    fn help_text_lists_check() {
        assert!(USAGE.contains("certcheck check"));
        assert!(USAGE.contains("SOLVER-FREE"));
    }

    #[test]
    fn explain_renders_a_claim() {
        // Smoke: fmt_formula over an obligation with a var doesn't panic.
        let o = Obligation {
            id: "x".into(),
            vars: vec![Var {
                name: "g".into(),
                sort: Sort::Int,
            }],
            claim: cmp(Op::Le, ivar("g"), int(5)),
        };
        assert!(fmt_formula(&o.claim).contains("g \u{2264} 5"));
    }
}
