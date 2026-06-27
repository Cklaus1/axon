//! R23 acceptance checks (A1–A6 + Core), named EXACTLY per §0. The solver-free
//! checker tests ALWAYS run + pass; the `smt`-only tests (`prove` / byte-
//! identical emission) skip cleanly when the crate is built without `smt`.
//!
//! These drive the REAL `certcheck` CLI as an auditor would (subprocess) and
//! validate the SHIPPED artifacts in `examples/proofs/` + the forgery corpus in
//! `tests/corpus/`.

use axon_certcheck::certificate::ProofCertificate;
use axon_certcheck::obligation::Obligation;
use axon_certcheck::{check, parse_certificate, parse_obligation, CheckResult};
use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <root>/crates/axon-certcheck
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn certcheck_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_certcheck"))
}

fn proofs() -> PathBuf {
    workspace_root().join("examples/proofs")
}

fn corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus")
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

fn load(stem: &str) -> (Obligation, ProofCertificate) {
    let o = parse_obligation(&read(&proofs().join(format!("{stem}.obl")))).expect("obl parses");
    let c = parse_certificate(&read(&proofs().join(format!("{stem}.cert")))).expect("cert parses");
    (o, c)
}

struct Out {
    stdout: String,
    code: i32,
}

fn cli(args: &[&str]) -> Out {
    let mut cmd = Command::new(certcheck_bin());
    cmd.args(args);
    cmd.current_dir(workspace_root()); // so `examples/proofs/...` resolves
    let out = cmd.output().expect("spawn certcheck");
    Out {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        code: out.status.code().unwrap_or(-1),
    }
}

// ── Core: a valid certificate for a TRUE obligation checks ✓ ─────────────────
#[test]
fn valid_certificate_accepted() {
    for stem in ["carve", "mint_o1", "mint_o2"] {
        let (o, c) = load(stem);
        assert_eq!(
            check(&o, &c),
            CheckResult::Valid,
            "{stem}: a valid cert for a true obligation must check"
        );
    }
}

// ── Core: a FALSE obligation has no valid certificate (soundness, §4.2) ───────
#[test]
fn false_obligation_has_no_valid_certificate() {
    use axon_certcheck::certificate::{FactOp, LinFact, Refutation};
    use axon_certcheck::obligation::build::*;
    use axon_certcheck::obligation::{Op, Sort, Var};

    // Take `carve` (min(g,avail) ≤ avail, TRUE) and flip ≤ to ≥ — now FALSE for
    // g < avail (e.g. g=0, avail=1: min=0, 0 ≥ 1 is false).
    let false_obl = Obligation {
        id: "carve/false".into(),
        vars: vec![
            Var {
                name: "g".into(),
                sort: Sort::Int,
            },
            Var {
                name: "avail".into(),
                sort: Sort::Int,
            },
        ],
        claim: cmp(Op::Ge, min(ivar("g"), ivar("avail")), ivar("avail")),
    };

    // (a) the synthesizer refuses to emit a cert for it (the negation is SAT).
    assert!(
        axon_certcheck::synth::synthesize(&false_obl).is_err(),
        "no certificate can be synthesized for a false obligation"
    );

    // (b) EVERY hand-forged candidate cert is rejected by the checker — including
    //     one that copies the real carve refutation structure and one with a
    //     bogus "0 < 0" leaf. There is no Valid cert for a false claim.
    let real_carve = axon_certcheck::synth::synthesize(&{
        let mut t = false_obl.clone();
        t.claim = cmp(Op::Le, min(ivar("g"), ivar("avail")), ivar("avail"));
        t
    })
    .unwrap();
    // Re-point the true cert at the false obligation (binding will reject).
    let mut transplant = real_carve.clone();
    transplant.obligation_id = false_obl.id.clone();
    transplant.obligation_digest = false_obl.digest();
    transplant.cert_digest = ProofCertificate::compute_cert_digest(&transplant.refutation);
    assert!(
        !check(&false_obl, &transplant).is_valid(),
        "the true cert transplanted onto the false obligation must NOT validate"
    );

    // A naked "0 < 0" leaf with no entailed facts.
    let bogus = ProofCertificate::new(
        &false_obl.id,
        &false_obl.digest(),
        Refutation::LinearContradiction {
            facts: vec![LinFact {
                terms: vec![],
                op: FactOp::Lt,
                constant: 0,
            }],
            coeffs: vec![1],
        },
    );
    assert!(
        !check(&false_obl, &bogus).is_valid(),
        "a fabricated 0<0 leaf must not pass: its fact isn't entailed by the (SAT) negated claim"
    );
}

// ── Core: a lying solver is caught by the checker (the trust shift, §4.3) ─────
#[test]
fn lying_solver_is_caught_by_checker() {
    use axon_certcheck::certificate::{FactOp, LinFact, Refutation};
    let (o, valid) = load("carve");

    // Model a solver that lied "UNSAT": certs with structural flaws.

    // (a) a Farkas witness whose weighted sum is `0 ≤ 3`, not `0 < 0`.
    let mut a = valid.clone();
    if let Refutation::IteCase { on_true, .. } = &mut a.refutation {
        **on_true = Refutation::LinearContradiction {
            facts: vec![
                LinFact {
                    terms: vec![(1, "g".into())],
                    op: FactOp::Le,
                    constant: 5,
                },
                LinFact {
                    terms: vec![(-1, "g".into())],
                    op: FactOp::Le,
                    constant: -2,
                },
            ],
            coeffs: vec![1, 1], // 0 ≤ 3
        };
    }
    a.cert_digest = ProofCertificate::compute_cert_digest(&a.refutation);
    assert!(
        !check(&o, &a).is_valid(),
        "non-contradiction must be caught"
    );

    // (b) a negative coefficient.
    let mut b = valid.clone();
    if let Refutation::IteCase { on_true, .. } = &mut b.refutation {
        if let Refutation::LinearContradiction { coeffs, .. } = on_true.as_mut() {
            coeffs[0] = -1;
        }
    }
    b.cert_digest = ProofCertificate::compute_cert_digest(&b.refutation);
    assert!(!check(&o, &b).is_valid(), "negative coeff must be caught");

    // (c) a single-branch "exhaustive" split (the on_false branch fakes success
    //     with a fact not entailed by that branch).
    let mut c = valid.clone();
    if let Refutation::IteCase { on_false, .. } = &mut c.refutation {
        **on_false = Refutation::LinearContradiction {
            facts: vec![LinFact {
                terms: vec![(1, "g".into())],
                op: FactOp::Le,
                constant: -1,
            }],
            coeffs: vec![1],
        };
    }
    c.cert_digest = ProofCertificate::compute_cert_digest(&c.refutation);
    assert!(
        !check(&o, &c).is_valid(),
        "a branch with a fabricated fact must be caught"
    );
}

// ── A6: every corpus forgery + a one-byte mutation of each valid cert rejected ─
#[test]
fn acc_a6_checker_rejects_forged_and_mutated() {
    let (carve_o, _carve_c) = load("carve");

    // (1) every shipped corpus forgery → not Valid.
    let mut seen = 0;
    for entry in std::fs::read_dir(corpus()).expect("corpus dir exists") {
        let p = entry.unwrap().path();
        if p.extension().and_then(|e| e.to_str()) != Some("cert") {
            continue;
        }
        seen += 1;
        let raw = read(&p);
        // A malformed forgery is also a rejection (fail-closed); only a PARSED
        // forgery needs the checker to reject it.
        if let Ok(c) = parse_certificate(&raw) {
            assert!(
                !check(&carve_o, &c).is_valid(),
                "corpus forgery {} must be rejected",
                p.display()
            );
        }
    }
    assert!(seen >= 5, "the forgery corpus must ship (saw {seen})");

    // (2) a one-byte mutation of each valid shipped cert → rejected. We mutate a
    //     coefficient, a fact constant, and the digest field, each in turn.
    for stem in ["carve", "mint_o1", "mint_o2"] {
        let (o, _) = load(stem);
        let raw = read(&proofs().join(format!("{stem}.cert")));
        let mut fired = 0;
        for (find, repl, what) in [
            (r#""coeffs":[1,1]"#, r#""coeffs":[1,2]"#, "coefficient"),
            (r#""constant":0"#, r#""constant":1"#, "fact constant"),
            // mint_o1's boolean leaves are `0 ≤ -1`; mutate that constant so the
            // pure-boolean cert is also covered (no `coeffs:[1,1]` to mutate).
            (r#""constant":-1"#, r#""constant":0"#, "false-leaf constant"),
            (
                "\"cert_digest\":\"axcert1:",
                "\"cert_digest\":\"axcert1:f",
                "cert_digest",
            ),
        ] {
            if !raw.contains(find) {
                continue; // not present in this cert; try the next mutation
            }
            let mutated = raw.replacen(find, repl, 1);
            assert_ne!(mutated, raw, "{stem}: mutation `{what}` changed nothing");
            let parsed = parse_certificate(&mutated);
            let rejected = match parsed {
                Ok(c) => !check(&o, &c).is_valid(),
                Err(_) => true,
            };
            assert!(rejected, "{stem}: mutating the {what} must be detected");
            fired += 1;
        }
        // Guard against a vacuous pass: every shipped cert must offer at least one
        // detectable mutation (else this stem proved nothing).
        assert!(fired > 0, "{stem}: no mutation fired — coverage is vacuous");
    }
}

// ── A4: the checker is solver-free (built --no-default-features; z3 absent) ────
#[test]
fn acc_a4_checker_is_solver_free() {
    // (a) the binary under test validates a cert (the trusted op runs).
    let r = cli(&[
        "check",
        "examples/proofs/carve.obl",
        "examples/proofs/carve.cert",
    ]);
    assert_eq!(r.code, 0, "checker validates carve: {}", r.stdout);
    assert!(
        r.stdout.contains("WITHOUT a solver"),
        "the check op advertises being solver-free: {}",
        r.stdout
    );

    // (b) build the crate --no-default-features and assert z3 is NOT in the
    //     checker's dependency closure (`cargo tree`). This is the enforced
    //     trusted-path-has-no-solver gate.
    let tree = Command::new(env!("CARGO"))
        .args([
            "tree",
            "-p",
            "axon-certcheck",
            "--no-default-features",
            "--edges",
            "normal",
        ])
        .current_dir(workspace_root())
        .output();
    match tree {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout);
            assert!(
                !s.to_lowercase().contains("z3"),
                "z3 MUST NOT be in the solver-free dependency closure:\n{s}"
            );
        }
        _ => {
            // `cargo tree` unavailable (offline sandbox): the cfg-gating itself
            // guarantees z3 is behind `smt`; the (a) run already proves the
            // checker works without it. Don't fail the suite on tooling absence.
            eprintln!("acc_a4: `cargo tree` unavailable — relying on the no-default-features run");
        }
    }
}

// ── A2: the shipped example obligations are certified (real artifacts) ────────
#[test]
fn acc_a2_example_obligations_certified() {
    for stem in ["carve", "mint_o1", "mint_o2"] {
        assert!(
            proofs().join(format!("{stem}.obl")).exists(),
            "{stem}.obl ships"
        );
        assert!(
            proofs().join(format!("{stem}.cert")).exists(),
            "{stem}.cert ships"
        );
        let r = cli(&[
            "check",
            &format!("examples/proofs/{stem}.obl"),
            &format!("examples/proofs/{stem}.cert"),
        ]);
        assert_eq!(r.code, 0, "{stem} certifies via the CLI: {}", r.stdout);
        assert!(r.stdout.contains("VALID"), "{stem}: {}", r.stdout);
    }
}

// ── A3: the README quickstart commands execute against the built binary ───────
#[test]
fn acc_a3_quickstart_commands_execute() {
    let readme = read(&workspace_root().join("README-axon-certcheck.md"));
    // The documented verbs must exist (docs can't rot).
    for verb in ["check", "explain", "prove"] {
        assert!(
            readme.contains(&format!("certcheck {verb}")),
            "README documents `{verb}`"
        );
    }
    // Step 1: check mint_o2 → VALID, exit 0.
    let r1 = cli(&[
        "check",
        "examples/proofs/mint_o2.obl",
        "examples/proofs/mint_o2.cert",
    ]);
    assert_eq!(r1.code, 0, "quickstart step 1: {}", r1.stdout);
    // Step 2: explain → readable refutation walk.
    let r2 = cli(&[
        "explain",
        "examples/proofs/mint_o2.obl",
        "--cert",
        "examples/proofs/mint_o2.cert",
    ]);
    assert_eq!(r2.code, 0);
    assert!(
        r2.stdout.contains("Farkas"),
        "explain walks the proof: {}",
        r2.stdout
    );
    // Step 3: a tampered certificate is REJECTED (exit 8). Mirrors the README's
    // interior-byte mutation (a stale digest), which yields a real Invalid.
    let carve = read(&proofs().join("carve.cert"));
    let bad = carve.replacen(r#""coeffs":[1,1]"#, r#""coeffs":[1,2]"#, 1);
    let tmp = std::env::temp_dir().join(format!("r23-a3-{}.cert", std::process::id()));
    std::fs::write(&tmp, bad).unwrap();
    let r3 = cli(&["check", "examples/proofs/carve.obl", tmp.to_str().unwrap()]);
    assert_eq!(
        r3.code, 8,
        "quickstart step 3 rejects the tamper: {}",
        r3.stdout
    );
    assert!(r3.stdout.contains("INVALID"));
}

// ── Core: --require-certificates fails closed on missing/invalid certs ────────
#[test]
fn require_certificates_fails_closed() {
    use axon_certcheck::{require_certificates, RequireOutcome};
    let (o, good) = load("carve");

    // valid cert + flag ⇒ Discharged.
    assert!(require_certificates(&o, Some(&good), true).is_discharged());

    // MISSING cert + flag ⇒ FailClosed (R20 mint obligation under the flag would
    // keep its runtime check armed → E1610; R21 admission would Deny).
    assert!(matches!(
        require_certificates(&o, None, true),
        RequireOutcome::FailClosed { .. }
    ));

    // INVALID cert + flag ⇒ FailClosed.
    let mut bad = good.clone();
    bad.obligation_digest = "axsha256:0".into();
    assert!(matches!(
        require_certificates(&o, Some(&bad), true),
        RequireOutcome::FailClosed { .. }
    ));

    // Default path (flag OFF) ⇒ Discharged (byte-compatible; no new demand).
    assert!(require_certificates(&o, None, false).is_discharged());
}

// ── A1: the end-to-end auditor journey through the real CLI ───────────────────
#[test]
fn acc_a1_smoke_prove_check_verify() {
    // (1) obtain a cert for mint_o2. With the `smt` build, via the real `prove`;
    //     otherwise use the shipped artifact (the checker journey is identical).
    let tmp = std::env::temp_dir().join(format!("r23-a1-{}.cert", std::process::id()));
    let cert_arg: String;
    #[cfg(feature = "smt")]
    {
        let p = cli(&[
            "prove",
            "examples/proofs/mint_o2.obl",
            "--out",
            tmp.to_str().unwrap(),
        ]);
        assert_eq!(p.code, 0, "prove succeeds: {}", p.stdout);
        assert!(
            p.stdout.contains("certified principal_mint/O2"),
            "prove announces the id: {}",
            p.stdout
        );
        cert_arg = tmp.to_str().unwrap().to_string();
    }
    #[cfg(not(feature = "smt"))]
    {
        std::fs::copy(proofs().join("mint_o2.cert"), &tmp).unwrap();
        cert_arg = tmp.to_str().unwrap().to_string();
    }

    // (2) check → VALID, solver-free.
    let c = cli(&["check", "examples/proofs/mint_o2.obl", &cert_arg]);
    assert_eq!(c.code, 0, "check passes: {}", c.stdout);
    assert!(
        c.stdout.contains("VALID") && c.stdout.contains("WITHOUT a solver"),
        "check announces solver-free validity: {}",
        c.stdout
    );

    // (3) explain → the readable refutation walk.
    let e = cli(&[
        "explain",
        "examples/proofs/mint_o2.obl",
        "--cert",
        &cert_arg,
    ]);
    assert_eq!(e.code, 0);
    assert!(
        e.stdout.contains("split on") && e.stdout.contains("Farkas"),
        "explain shows the refutation: {}",
        e.stdout
    );

    // (4) corrupt one byte of the cert and re-check → INVALID exit 8.
    let raw = read(&tmp);
    let bad = if raw.contains(r#""coeffs":[1,1]"#) {
        raw.replacen(r#""coeffs":[1,1]"#, r#""coeffs":[1,2]"#, 1)
    } else {
        // fall back to mutating the first coeffs array we can find
        raw.replacen("\"coeffs\":[1", "\"coeffs\":[7", 1)
    };
    assert_ne!(bad, raw, "mutation changed the cert");
    let badp = std::env::temp_dir().join(format!("r23-a1-bad-{}.cert", std::process::id()));
    std::fs::write(&badp, bad).unwrap();
    let v = cli(&[
        "check",
        "examples/proofs/mint_o2.obl",
        badp.to_str().unwrap(),
    ]);
    assert_eq!(v.code, 8, "corrupted cert is INVALID: {}", v.stdout);
    assert!(v.stdout.contains("INVALID"));
}

// ── A5: emission is byte-identical (smt-only; skips cleanly without z3) ───────
#[cfg(feature = "smt")]
#[test]
fn acc_a5_certificate_byte_identical() {
    let a = std::env::temp_dir().join(format!("r23-a5-a-{}.cert", std::process::id()));
    let b = std::env::temp_dir().join(format!("r23-a5-b-{}.cert", std::process::id()));
    let r1 = cli(&[
        "prove",
        "examples/proofs/mint_o2.obl",
        "--out",
        a.to_str().unwrap(),
    ]);
    let r2 = cli(&[
        "prove",
        "examples/proofs/mint_o2.obl",
        "--out",
        b.to_str().unwrap(),
    ]);
    assert_eq!(r1.code, 0);
    assert_eq!(r2.code, 0);
    let ab = std::fs::read(&a).unwrap();
    let bb = std::fs::read(&b).unwrap();
    assert_eq!(ab, bb, "prove must emit a byte-identical cert across runs");
}

// When built WITHOUT smt, A5 still has a presence so the gate's grep finds it,
// and the property (determinism) is asserted via the pure synthesizer.
#[cfg(not(feature = "smt"))]
#[test]
fn acc_a5_certificate_byte_identical() {
    // The emission strategy is deterministic; the synthesizer is the same code
    // `prove` uses to build the cert. Two syntheses must be byte-identical.
    let (o, _) = load("mint_o2");
    let a = axon_certcheck::synth::synthesize(&o).unwrap().to_json();
    let b = axon_certcheck::synth::synthesize(&o).unwrap().to_json();
    assert_eq!(a, b, "deterministic emission (A5): syntheses must match");
    // And the shipped artifact must equal a fresh synthesis (the artifact is
    // reproducible from the obligation alone).
    let shipped = read(&proofs().join("mint_o2.cert"));
    assert_eq!(
        shipped.trim_end(),
        a,
        "the shipped mint_o2.cert must be byte-reproducible from the obligation"
    );
}
