//! `@[forall]` property-testing harness for the interpreter (R0 slice 4 —
//! extracted verbatim from `interp.rs`, zero behavior change). Generates
//! seeded-random typed inputs for a `@[test] @[forall]` fn, runs the body
//! each case, and on the first failure SHRINKS toward zero/empty to report a
//! minimal counterexample (R8). Orthogonal to interpretation itself.
//! `use super::*` pulls in Interp/Program/FnDef/Value/Flow, the RNG
//! (next_rand_u64), display, on_deep_stack, and is_i64_type/is_f64_type.

use super::*;

/// Outcome of a `forall` property test (R8).
pub enum PropertyOutcome {
    /// All `cases` random inputs passed.
    Passed { cases: u32 },
    /// A case failed; `counterexample` is the SHRUNK minimal input (rendered),
    /// `message` is the assertion/panic message, `seed` reproduces the run.
    Failed {
        counterexample: String,
        message: String,
        seed: u64,
    },
    /// The property fn has an unsupported param type / shape.
    Unsupported(String),
}

/// Run a `@[test] @[forall]` property test: generate `cases` random argument
/// tuples for `name`'s typed params (seeded RNG — reproducible via `AXON_SEED`),
/// run the body each time, and on the first failure SHRINK the inputs toward
/// zero/empty to report a minimal counterexample (R8). The interpreter is the
/// reference; a body failure is a `Flow::Panic` (from `assert`/`assert_eq`).
pub fn run_property_test(program: &Program, name: &str, cases: u32) -> PropertyOutcome {
    on_deep_stack(|| run_property_test_inner(program, name, cases))
}

pub(super) fn run_property_test_inner(
    program: &Program,
    name: &str,
    cases: u32,
) -> PropertyOutcome {
    let seed = rng_seed();
    let mut interp = Interp::build(program);
    if let Err(f) = interp.init_globals() {
        return PropertyOutcome::Unsupported(flow_to_msg(f));
    }
    let Some(f) = interp.fns.get(name).copied() else {
        return PropertyOutcome::Unsupported(format!("no function `{name}`"));
    };
    // Generators per param type; bail out if any param isn't supported.
    let gens: Vec<PropGen> = match f
        .params
        .iter()
        .map(|p| prop_gen_for(&p.ty))
        .collect::<Option<Vec<_>>>()
    {
        Some(g) if !g.is_empty() => g,
        Some(_) => {
            return PropertyOutcome::Unsupported(
                "forall property test needs at least one parameter".into(),
            )
        }
        None => {
            return PropertyOutcome::Unsupported(
                "forall supports i64/f64/bool/str parameters only".into(),
            )
        }
    };

    // Try `cases` random inputs; on the first failing one, shrink it.
    for _ in 0..cases {
        let args: Vec<Value> = gens.iter().map(|g| g.random()).collect();
        if let Err(msg) = run_once(&interp, f, &args) {
            // Found a failing case — shrink toward minimal.
            let (shrunk_args, shrunk_msg) = shrink(&interp, f, &gens, args, msg);
            let ce = render_args(&f.params, &shrunk_args);
            return PropertyOutcome::Failed {
                counterexample: ce,
                message: shrunk_msg,
                seed,
            };
        }
    }
    PropertyOutcome::Passed { cases }
}

/// Run the property fn once with `args`; Ok(()) if it passed (no panic),
/// Err(message) if an assert/panic fired.
pub(super) fn run_once(interp: &Interp, f: &FnDef, args: &[Value]) -> Result<(), String> {
    match interp.call_fn(f, args.to_vec()) {
        Ok(_) => Ok(()),
        Err(Flow::Panic(m)) | Err(Flow::VerifyFailed(m)) => Err(m),
        // A stray return/exit is treated as a pass (the assert didn't fire).
        Err(_) => Ok(()),
    }
}

/// Shrink each argument toward its minimal failing value while the property
/// still fails. For ints/floats this BINARY-SEARCHES toward zero (so a property
/// failing at `a >= 50` shrinks to exactly `a = 50`, not just "some smaller
/// failing value"); strings truncate; bool flips true→false. Returns the
/// minimal failing args + the message from that minimal case.
pub(super) fn shrink(
    interp: &Interp,
    f: &FnDef,
    gens: &[PropGen],
    start: Vec<Value>,
    start_msg: String,
) -> (Vec<Value>, String) {
    let mut best = start;
    let mut best_msg = start_msg;
    // A few passes let later-arg shrinks re-enable earlier-arg shrinks.
    for _ in 0..4 {
        let mut improved = false;
        for i in 0..best.len() {
            // For ints/floats, binary-search this arg to the exact failing
            // boundary: `fail` is the current failing value, `pass` a value
            // toward zero that passes (0 if even 0 fails). Repeatedly probe the
            // midpoint, keeping the failing side, until adjacent.
            if matches!(gens[i], PropGen::I64 | PropGen::F64) {
                // Establish a passing bound toward zero (try 0 first).
                let zero = gens[i].zero();
                let mut t0 = best.clone();
                t0[i] = zero.clone();
                let (mut pass, zero_fails) = match run_once(interp, f, &t0) {
                    Ok(()) => (Some(zero), false),
                    Err(m) => {
                        best = t0;
                        best_msg = m;
                        improved = true;
                        (None, true)
                    }
                };
                if !zero_fails {
                    // Binary-search between `pass` (toward 0) and best[i] (fails).
                    while let Some(mid) = gens[i].step_between(pass.as_ref().unwrap(), &best[i]) {
                        let mut trial = best.clone();
                        trial[i] = mid.clone();
                        match run_once(interp, f, &trial) {
                            Err(m) => {
                                best = trial;
                                best_msg = m;
                                improved = true;
                            }
                            Ok(()) => {
                                pass = Some(mid);
                            }
                        }
                    }
                }
            } else {
                // bool/str: greedy single-direction shrink.
                while let Some(candidate) = gens[i].shrink_toward(&best[i]) {
                    let mut trial = best.clone();
                    trial[i] = candidate;
                    match run_once(interp, f, &trial) {
                        Err(m) => {
                            best = trial;
                            best_msg = m;
                            improved = true;
                        }
                        Ok(()) => break,
                    }
                }
            }
        }
        if !improved {
            break;
        }
    }
    (best, best_msg)
}

/// A generator+shrinker for one property parameter type.
pub(super) enum PropGen {
    I64,
    F64,
    Bool,
    Str,
}

pub(super) fn prop_gen_for(ty: &crate::ast::AxonType) -> Option<PropGen> {
    if is_i64_type(ty) {
        Some(PropGen::I64)
    } else if is_f64_type(ty) {
        Some(PropGen::F64)
    } else if matches!(ty, crate::ast::AxonType::Named(n) if n == "bool") {
        Some(PropGen::Bool)
    } else if matches!(ty, crate::ast::AxonType::Named(n) if n == "str") {
        Some(PropGen::Str)
    } else {
        None
    }
}

impl PropGen {
    fn random(&self) -> Value {
        match self {
            // Bias toward small magnitudes (good property-test inputs) but cover
            // the full i64 range occasionally.
            PropGen::I64 => {
                let r = next_rand_u64();
                let v = if r & 7 == 0 {
                    r as i64
                } else {
                    (r % 201) as i64 - 100
                };
                Value::Int(v)
            }
            PropGen::F64 => {
                let r = next_rand_u64();
                Value::Float((r % 2001) as f64 / 100.0 - 10.0)
            }
            PropGen::Bool => Value::Bool(next_rand_u64() & 1 == 0),
            PropGen::Str => {
                let len = (next_rand_u64() % 8) as usize;
                let s: String = (0..len)
                    .map(|_| (b'a' + (next_rand_u64() % 26) as u8) as char)
                    .collect();
                Value::Str(s)
            }
        }
    }

    /// The minimal value for this type (the binary-search target).
    fn zero(&self) -> Value {
        match self {
            PropGen::I64 => Value::Int(0),
            PropGen::F64 => Value::Float(0.0),
            PropGen::Bool => Value::Bool(false),
            PropGen::Str => Value::Str(String::new()),
        }
    }

    /// A big step toward the minimal value (halve int/float toward 0, flip
    /// bool, drop a char), or None if already minimal.
    fn shrink_toward(&self, v: &Value) -> Option<Value> {
        match (self, v) {
            (PropGen::I64, Value::Int(0)) => None,
            (PropGen::I64, Value::Int(n)) => Some(Value::Int(n / 2)),
            (PropGen::F64, Value::Float(f)) if *f == 0.0 => None,
            (PropGen::F64, Value::Float(f)) => {
                Some(Value::Float((f / 2.0 * 100.0).round() / 100.0))
            }
            (PropGen::Bool, Value::Bool(true)) => Some(Value::Bool(false)),
            (PropGen::Bool, Value::Bool(false)) => None,
            (PropGen::Str, Value::Str(s)) if s.is_empty() => None,
            (PropGen::Str, Value::Str(s)) => Some(Value::Str(s[..s.len() - 1].to_string())),
            _ => None,
        }
    }

    /// The midpoint between a known-passing value `from` and a known-failing
    /// value `to` — used to binary-search to the exact failing boundary (so
    /// `a >= 50` shrinks to `a = 50`). Only meaningful for ints/floats; None
    /// when adjacent or for non-numeric types.
    fn step_between(&self, from: &Value, to: &Value) -> Option<Value> {
        match (self, from, to) {
            (PropGen::I64, Value::Int(a), Value::Int(b)) => {
                let mid = a + (b - a) / 2;
                if mid == *a || mid == *b {
                    None
                } else {
                    Some(Value::Int(mid))
                }
            }
            (PropGen::F64, Value::Float(a), Value::Float(b)) => {
                let mid = ((a + (b - a) / 2.0) * 100.0).round() / 100.0;
                if (mid - a).abs() < 0.01 || (mid - b).abs() < 0.01 {
                    None
                } else {
                    Some(Value::Float(mid))
                }
            }
            _ => None,
        }
    }
}

pub(super) fn render_args(params: &[crate::ast::Param], args: &[Value]) -> String {
    params
        .iter()
        .zip(args)
        .map(|(p, v)| format!("{}={}", p.name, display(v)))
        .collect::<Vec<_>>()
        .join(", ")
}
