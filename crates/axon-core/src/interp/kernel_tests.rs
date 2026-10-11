//! R51 S3: the per-frame `Kernel` split (TaintPort-7; v0.22 interp.rs:708-842,
//! 3933-3960). A sealed frame's handle-addressed state — principals, the
//! scheduler, stores, sandboxes, the provenance store, the corrigibility latch
//! and the RNG stream — is its own kernel's, disjoint from the operator's.
//!
//! Every sealed run is refused at build until S6 (`SEAL_RUNTIME_COMPLETE`), and
//! no seal edge enters a sealed frame yet, so these drive an `Interp` made by
//! [`Interp::build_with`] (seal state computed, refuse-all pin skipped, as
//! `taint_tests` does) and enter frames through [`Interp::with_frame`], the one
//! place provenance changes.

use super::*;

const SUITE: &str = "fn w(n: i64) -> i64 { n }\n";
const CAND: &str = "fn solve() -> i64 { 41 }\n";

/// The suite and a sealed candidate, as one program, and the sealed directory.
fn prog(tag: &str) -> (Program, std::path::PathBuf) {
    use crate::span::intern_source;
    let sdir = format!("/{tag}-sealed");
    let s = crate::parse_source_in(SUITE, intern_source(&format!("/{tag}-suite/h.ax"), SUITE))
        .expect("suite parses");
    let c = crate::parse_source_in(CAND, intern_source(&format!("{sdir}/f.ax"), CAND))
        .expect("candidate parses");
    let prog = Program {
        items: s.items.into_iter().chain(c.items).collect(),
    };
    (prog, std::path::PathBuf::from(sdir))
}

fn with_sealed<R>(tag: &str, f: impl FnOnce(&Interp<'_>) -> R) -> R {
    let (prog, sdir) = prog(tag);
    let ix = Interp::build_with(&prog, std::slice::from_ref(&sdir), Engine::Tree);
    assert!(ix.seal.active);
    f(&ix)
}

/// `name(args)` through the builtin dispatcher, in whatever frame is running.
fn call(ix: &Interp<'_>, name: &str, args: &[Value]) -> Result<Value, Flow> {
    ix.call_builtin(name, args)
        .map(|v| v.unwrap_or_else(|| panic!("`{name}` is a builtin")))
}

fn ok(ix: &Interp<'_>, name: &str, args: &[Value]) -> Value {
    call(ix, name, args).unwrap_or_else(|e| panic!("{name}: {e:?}"))
}

fn int(ix: &Interp<'_>, name: &str, args: &[Value]) -> i64 {
    match ok(ix, name, args) {
        Value::Int(n) => n,
        v => panic!("{name}: {v:?}"),
    }
}

fn s(x: &str) -> Value {
    Value::Str(Rc::new(x.to_string()))
}

/// The kernel the running frame reaches.
fn reached(ix: &Interp<'_>) -> usize {
    let k = ix.k() as *const Kernel;
    (0..2)
        .find(|&i| std::ptr::eq(k, &ix.kernels[i]))
        .expect("k() is one of the two kernels")
}

/// `k()` selects by the running frame's provenance, through nesting; the
/// sealed kernel's stream and token registry are the sealed ones.
#[test]
fn the_running_frame_selects_its_kernel() {
    with_sealed("r51-s3a-k", |ix| {
        assert_eq!(reached(ix), 0, "the operator's frame");
        let seen = ix.with_frame(true, || {
            let inner = ix.with_frame(false, || reached(ix));
            (reached(ix), inner)
        });
        assert_eq!(seen, (1, 0), "sealed frame, then operator code above it");
        assert_eq!(reached(ix), 0, "restored after the frame");
        assert!(!ix.kernels[0].rng_sealed && ix.kernels[1].rng_sealed);
        assert!(!ix.kernels[0].principals.borrow().is_sealed());
        assert!(
            ix.kernels[1].principals.borrow().is_sealed(),
            "the sealed kernel's registry draws from TOKEN_DOMAIN_SEALED"
        );
    });
}

/// The PCI candidate-4 attacks: an operator handle does not exist from a
/// sealed frame, and nothing a sealed frame does to its kernel reaches the
/// operator's — principals and their budgets, the scheduler's fibers, the
/// sandboxes, the provenance store, the corrigibility latch, the active
/// principal.
#[test]
fn a_sealed_frames_kernel_state_is_disjoint_from_the_operators() {
    with_sealed("r51-s3a-disjoint", |ix| {
        // The operator's state.
        let op = int(
            ix,
            "principal_root",
            &[
                s("op"),
                Value::Bool(true),
                Value::Bool(true),
                Value::Bool(true),
                Value::Int(100),
            ],
        );
        assert_eq!(
            int(ix, "principal_budget_remaining", &[Value::Int(op)]),
            100
        );
        ok(ix, "principal_activate", &[Value::Int(op)]);
        assert_eq!(int(ix, "scheduler_spawn", &[s("w"), Value::Int(1)]), 0);
        ok(ix, "corrigible_halt", &[]);
        ix.kernels[0]
            .provenance
            .borrow_mut()
            .insert("w".to_string(), vec![1.0, 2.0]);
        let sb = int(ix, "sandbox_create", &[Value::Int(op), s("AI")]);
        let op_sandboxes = ix.kernels[0].sandboxes.borrow().len();

        // The sealed frame sees none of it, and writes only its own.
        let cand = ix.with_frame(true, || {
            assert_eq!(
                int(ix, "principal_budget_remaining", &[Value::Int(op)]),
                0,
                "an operator principal handle is unknown in a sealed frame"
            );
            assert_eq!(
                int(ix, "principal_spend", &[Value::Int(op), Value::Int(60)]),
                0,
                "a sealed frame cannot spend an operator principal"
            );
            assert!(
                matches!(ok(ix, "principal_current_name", &[]), Value::Str(n) if *n != "op"),
                "the operator's active principal is not the sealed frame's"
            );
            assert!(matches!(
                ok(ix, "corrigible_halted", &[]),
                Value::Bool(false)
            ));
            assert_eq!(int(ix, "goal_count", &[s("w")]), 0, "the provenance store");
            assert_eq!(int(ix, "scheduler_done_count", &[]), 0);
            assert!(
                call(ix, "sandbox_run", &[Value::Int(sb), s("w"), Value::Int(3)]).is_err(),
                "an operator sandbox handle is unknown in a sealed frame"
            );
            // A sealed frame queues only its own fns (the call edge at
            // resolution, `fn_by_name`, R51 S3).
            assert_eq!(int(ix, "scheduler_spawn", &[s("solve"), Value::Int(2)]), 0);
            ok(ix, "corrigible_halt", &[]);
            let c = int(
                ix,
                "principal_root",
                &[
                    s("cand"),
                    Value::Bool(true),
                    Value::Bool(true),
                    Value::Bool(true),
                    Value::Int(5),
                ],
            );
            ok(ix, "principal_activate", &[Value::Int(c)]);
            int(ix, "sandbox_create", &[Value::Int(c), s("AI")]);
            c
        });

        // The operator's state is as it left it.
        assert_eq!(
            int(ix, "principal_budget_remaining", &[Value::Int(op)]),
            100
        );
        assert_eq!(
            int(ix, "principal_budget_remaining", &[Value::Int(cand)]),
            0,
            "a sealed principal handle is unknown to the operator"
        );
        assert!(matches!(ok(ix, "principal_current_name", &[]), Value::Str(n) if *n == "op"));
        assert_eq!(ix.kernels[0].scheduler.borrow().len(), 1, "operator fibers");
        assert_eq!(ix.kernels[1].scheduler.borrow().len(), 1, "sealed fibers");
        assert_eq!(int(ix, "goal_count", &[s("w")]), 2);
        assert_eq!(ix.kernels[0].sandboxes.borrow().len(), op_sandboxes);
        assert_eq!(
            ix.kernels[1].sandboxes.borrow().len(),
            op_sandboxes,
            "the sealed frame's sandbox is in its own kernel (both start at the ambient ceiling)"
        );
        assert!(ix.kernels[1].corrigible_halted.get());
        assert!(ix.kernels[0].principals.borrow().get(cand).is_none());
        assert!(ix.kernels[1].principals.borrow().get(op).is_none());
    });
}

/// A sealed frame draws from, and reseeds, only its own stream: `srand(7)`
/// there yields the xorshift64 sequence of seed 7 from the SEALED kernel's
/// cell, and the operator kernel's cell (unused: the operator's stream is the
/// process-global one) is never written.
#[test]
fn a_sealed_frame_draws_and_reseeds_only_its_own_stream() {
    with_sealed("r51-s3a-rng", |ix| {
        let draws = ix.with_frame(true, || {
            ok(ix, "srand", &[Value::Int(7)]);
            assert_eq!(ix.kernels[1].rng.get(), 7, "srand wrote the sealed cell");
            (0..6)
                .map(|_| int(ix, "random_i64", &[Value::Int(0), Value::Int(1000)]))
                .collect::<Vec<_>>()
        });
        assert_eq!(draws, [327, 652, 743, 107, 850, 125]);
        assert_ne!(
            ix.kernels[1].rng.get(),
            7,
            "the draws advanced the sealed cell"
        );
        assert_eq!(ix.kernels[0].rng.get(), 0, "the operator kernel's cell");
        // `srand(0)` maps to the non-zero sentinel, never "unseeded".
        ix.with_frame(true, || ok(ix, "srand", &[Value::Int(0)]));
        assert_eq!(ix.kernels[1].rng.get(), 1);
    });
}

/// An unseeded sealed stream starts from a one-way derivation of the
/// operator's seed: deterministic under `AXON_SEED`, never the seed itself,
/// never the "unseeded" `0`.
#[test]
fn the_sealed_stream_is_a_one_way_derivation_of_the_operator_seed() {
    for seed in [1u64, 7, 11, u64::MAX] {
        let d = sealed_rng_seed(seed);
        assert_eq!(d, sealed_rng_seed(seed), "deterministic");
        assert_ne!(d, seed);
        assert_eq!(d & 1, 1, "never 0");
    }
    assert_ne!(sealed_rng_seed(7), sealed_rng_seed(11));
}

/// C9 round 3 (PSV-1, amendment 46): operator code running ABOVE a sealed
/// frame (a closure the suite handed the candidate) runs as many times as the
/// candidate chooses, so it may neither draw from nor reseed the operator's
/// stream. Operator code with no sealed frame below it may.
#[test]
fn operator_code_above_a_sealed_frame_cannot_use_the_operators_stream() {
    with_sealed("r51-s3a-guard", |ix| {
        for (name, args) in [
            ("random_i64", vec![Value::Int(0), Value::Int(10)]),
            ("random_f64", vec![]),
            ("srand", vec![Value::Int(3)]),
        ] {
            let r = ix.with_frame(true, || ix.with_frame(false, || call(ix, name, &args)));
            match r {
                Err(Flow::Panic(m)) => assert!(
                    m.to_string()
                        .contains("the operator's random stream cannot be used"),
                    "{name}: {m}"
                ),
                other => panic!("{name}: operator code above a sealed frame drew: {other:?}"),
            }
            assert!(
                ix.with_frame(true, || call(ix, name, &args)).is_ok(),
                "{name}: the sealed frame itself draws its own"
            );
        }
        assert!(
            call(ix, "random_i64", &[Value::Int(0), Value::Int(10)]).is_ok(),
            "no sealed frame below"
        );
    });
}

/// `sandbox_run` from an operator frame of a sealed run copies the entered
/// sandbox into the sealed kernel for the life of the call (a sealed callee's
/// frame reads its own kernel's ceiling) and removes it after, restoring both
/// kernels' active handles.
#[test]
fn sandbox_run_restores_both_kernels_ceilings() {
    with_sealed("r51-s3a-sb", |ix| {
        let p = int(
            ix,
            "principal_root",
            &[
                s("op"),
                Value::Bool(true),
                Value::Bool(true),
                Value::Bool(true),
                Value::Int(100),
            ],
        );
        let sb = int(ix, "sandbox_create", &[Value::Int(p), s("AI")]);
        let before = (
            ix.kernels[0].active_sandbox.get(),
            ix.kernels[1].active_sandbox.get(),
            ix.kernels[1].sandboxes.borrow().len(),
        );
        assert_eq!(
            int(ix, "sandbox_run", &[Value::Int(sb), s("w"), Value::Int(9)]),
            9
        );
        assert_eq!(
            (
                ix.kernels[0].active_sandbox.get(),
                ix.kernels[1].active_sandbox.get(),
                ix.kernels[1].sandboxes.borrow().len(),
            ),
            before
        );
    });
}

/// An unsealed run never sets `frame_sealed`, so it only ever reaches
/// `kernels[0]`: the sealed kernel stays as built.
#[test]
fn an_unsealed_run_only_reaches_the_operator_kernel() {
    let (prog, _) = prog("r51-s3a-unsealed");
    let ix = Interp::build_with(&prog, &[], Engine::Tree);
    assert!(!ix.seal.active);
    assert_eq!(reached(&ix), 0);
    let p = int(
        &ix,
        "principal_root",
        &[
            s("op"),
            Value::Bool(true),
            Value::Bool(true),
            Value::Bool(true),
            Value::Int(100),
        ],
    );
    int(&ix, "sandbox_create", &[Value::Int(p), s("AI")]);
    int(&ix, "scheduler_spawn", &[s("w"), Value::Int(1)]);
    ok(&ix, "corrigible_halt", &[]);
    assert!(ix.kernels[0].principals.borrow().get(p).is_some());
    assert!(ix.kernels[1].principals.borrow().get(p).is_none());
    assert_eq!(ix.kernels[1].scheduler.borrow().len(), 0);
    assert!(!ix.kernels[1].corrigible_halted.get());
    assert_eq!(
        ix.kernels[1].sandboxes.borrow().len() + 1,
        ix.kernels[0].sandboxes.borrow().len()
    );
}

/// Loop finding 47 (v0.22 taint_tests.rs:2719-2769): a Kernel builtin that
/// draws from the RNG stream is coupled to the world cell `random_*` reads.
/// The goal fns that draw (`self.rng_next(` in `interp/goal.rs`) and every
/// builtin arm that reaches one must be in [`super::taint::RNG_COUPLED`].
#[test]
fn every_kernel_builtin_that_draws_the_rng_is_coupled_to_the_world_cell() {
    use super::taint::{info, Class, RNG_COUPLED};
    let sources = vm::tests::r51_interp_sources();
    let src = |f: &str| {
        sources
            .iter()
            .find(|(n, _)| n == f)
            .unwrap_or_else(|| panic!("interp/{f}"))
            .1
            .clone()
    };
    let mut drawers: Vec<String> = Vec::new();
    let mut cur = String::new();
    for l in src("goal.rs").lines() {
        let t = l.trim_start();
        let t2 = t
            .strip_prefix("pub(super) ")
            .or_else(|| t.strip_prefix("pub(crate) "))
            .unwrap_or(t);
        if let Some(r) = t2.strip_prefix("fn ") {
            cur = r
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
        }
        if l.contains("rng_next(") && !t.starts_with("//") {
            drawers.push(cur.clone());
        }
    }
    drawers.sort();
    drawers.dedup();
    assert!(
        drawers.len() >= 3,
        "DRIFT: goal.rs RNG drawers not found: {drawers:?}"
    );
    let mut coupled = 0;
    for (names, body) in super::tests::builtin_arms(&src("builtins.rs")) {
        if drawers.iter().any(|d| body.contains(&format!("self.{d}("))) {
            for n in names {
                if info(&n).is_some_and(|i| i.class == Class::Kernel) {
                    assert!(
                        RNG_COUPLED.contains(&n.as_str()),
                        "DRIFT: Kernel builtin `{n}` draws the RNG and is not in RNG_COUPLED"
                    );
                    coupled += 1;
                }
            }
        }
    }
    assert!(coupled >= 1, "DRIFT: no drawing Kernel builtin found");
    for n in RNG_COUPLED {
        assert_eq!(
            info(n).map(|i| i.class),
            Some(Class::Kernel),
            "RNG_COUPLED lists `{n}`, which is not a Kernel builtin"
        );
    }
}
