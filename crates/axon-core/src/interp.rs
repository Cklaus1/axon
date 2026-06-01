//! Tree-walking interpreter over the typed AST.
//!
//! This is the codegen-free execution path: it runs a parsed [`Program`]
//! directly, with no dependency on `inkwell`/LLVM. It exists so `axon run`
//! works end-to-end (and the Phase-10 `goal.md → goal.ax → result` story is
//! runnable) while the native codegen build is slow/blocked.
//!
//! For an LLM-orchestration workload the hot path is network latency, not
//! arithmetic, so a tree-walker is the appropriate execution model — native
//! codegen is a later performance concern, not a prerequisite.
//!
//! ## Scope (M1)
//! Implements the full deterministic core: scalars, arithmetic, `if`/`match`,
//! `let`/assignment, `while`/`while let`/`for`, user functions + recursion,
//! closures (lambdas with capture), structs, enum ADTs, `Option`/`Result`,
//! arrays, `?` propagation, string interpolation, impl-method dispatch, and a
//! broad builtin set (I/O, conversion, math, `str_*`, `assert*`).
//!
//! The ASI builtins (`ai_complete`, `ai_extract_*`, `goal_run`, the
//! `uncertain_*`/`temporal_*` family) are stubbed with a clear runtime error —
//! wiring them to `axon-ai`/`axon-rt` is M2.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io::Write as _;
use std::rc::Rc;

use crate::ast::{BinOp, EnumDef, Expr, FnDef, ImplBlock, Item, Literal, Pattern, Program, Stmt,
                 TypeDef, UnaryOp};

// ── Runtime values ────────────────────────────────────────────────────────────

/// A runtime value produced by evaluating an expression.
#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    Unit,
    Array(Vec<Value>),
    /// Structural record: `Point { x, y }`.
    Struct { name: String, fields: HashMap<String, Value> },
    /// Enum variant: `Shape::Circle { radius }`.
    Enum { enum_name: String, variant: String, fields: HashMap<String, Value> },
    Some(Box<Value>),
    None,
    Ok(Box<Value>),
    Err(Box<Value>),
    /// A lambda plus the environment it captured at creation time.
    Closure { params: Vec<String>, body: Box<Expr>, captured: HashMap<String, Value> },
    /// A channel — a shared FIFO queue. Cloning shares the same channel (Rc), so
    /// a `spawn`ed body and the main flow see the same queue. The interpreter is
    /// cooperative/single-threaded: `spawn` runs eagerly, so a `send` happens
    /// before the matching `recv`.
    Chan(Rc<RefCell<VecDeque<Value>>>),
    /// Tuple value `(a, b, …)`. Accessed via `t.0`, `t.1` (numeric field).
    Tuple(Vec<Value>),
    /// String-keyed dictionary — the ASI workhorse for caches, frequency
    /// tables, named state. Mutating builtins (`dict_set`, `dict_remove`)
    /// share the inner `RefCell` so a stored handle stays in sync with
    /// the live state, matching the channel model. Keys are `str` only
    /// (not arbitrary `Value`) — covers 95% of ASI use cases without
    /// requiring `Hash + Eq` on the full Value enum.
    Dict(Rc<RefCell<std::collections::BTreeMap<String, Value>>>),
}

impl Value {
    fn type_name(&self) -> String {
        match self {
            Value::Int(_) => "i64".into(),
            Value::Float(_) => "f64".into(),
            Value::Bool(_) => "bool".into(),
            Value::Str(_) => "str".into(),
            Value::Unit => "()".into(),
            Value::Array(_) => "[]".into(),
            Value::Struct { name, .. } => name.clone(),
            Value::Enum { enum_name, .. } => enum_name.clone(),
            Value::Some(_) | Value::None => "Option".into(),
            Value::Ok(_) | Value::Err(_) => "Result".into(),
            Value::Closure { .. } => "fn".into(),
            Value::Chan(_) => "chan".into(),
            Value::Tuple(_) => "tuple".into(),
            Value::Dict(_) => "dict".into(),
        }
    }
}

// ── Non-local control flow ──────────────────────────────────────────────────

/// A non-`Ok` outcome of evaluation. Normal values flow as `Ok(Value)`; these
/// are the ways evaluation can stop short of producing a value in-place.
#[derive(Debug)]
pub enum Flow {
    /// `return <expr>` — unwind to the enclosing function boundary.
    Return(Value),
    /// `break` — exit the nearest loop.
    Break,
    /// `continue` — skip to the next loop iteration.
    Continue,
    /// A runtime panic (failed assert, type error, OOB index, …).
    Panic(String),
    /// `exit(code)` — terminate the process with `code`.
    Exit(i32),
}

type R = Result<Value, Flow>;

fn panic<T>(msg: impl Into<String>) -> Result<T, Flow> {
    Err(Flow::Panic(msg.into()))
}

// ── Lexical environment ──────────────────────────────────────────────────────

/// A stack of lexical scopes. Innermost scope is last.
struct Env {
    scopes: Vec<HashMap<String, Value>>,
}

impl Env {
    fn new() -> Self {
        Env { scopes: vec![HashMap::new()] }
    }
    fn push(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn pop(&mut self) {
        self.scopes.pop();
    }
    fn define(&mut self, name: String, val: Value) {
        self.scopes.last_mut().unwrap().insert(name, val);
    }
    fn get(&self, name: &str) -> Option<&Value> {
        self.scopes.iter().rev().find_map(|s| s.get(name))
    }
    /// Update the nearest existing binding; returns false if none exists.
    fn assign(&mut self, name: &str, val: Value) -> bool {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), val);
                return true;
            }
        }
        false
    }
    /// Mutable reference to the nearest existing binding (for place assignment).
    fn get_mut(&mut self, name: &str) -> Option<&mut Value> {
        self.scopes.iter_mut().rev().find_map(|s| s.get_mut(name))
    }
    /// Flatten all visible bindings into one map (inner shadows outer).
    /// Used to snapshot the environment a closure captures.
    fn snapshot(&self) -> HashMap<String, Value> {
        let mut out = HashMap::new();
        for scope in &self.scopes {
            for (k, v) in scope {
                out.insert(k.clone(), v.clone());
            }
        }
        out
    }
}

// ── Interpreter ──────────────────────────────────────────────────────────────

pub struct Interp<'p> {
    fns: HashMap<String, &'p FnDef>,
    #[allow(dead_code)]
    structs: HashMap<String, &'p TypeDef>,
    #[allow(dead_code)]
    enums: HashMap<String, &'p EnumDef>,
    /// `(type_name, method_name) → method def` from impl blocks.
    methods: HashMap<(String, String), &'p FnDef>,
    /// Module-level `let NAME = …` constant definitions, in source order.
    global_defs: Vec<(String, &'p Expr)>,
    /// Evaluated module-level constants (populated by [`Interp::init_globals`]).
    globals: HashMap<String, Value>,
    /// In-memory provenance store: `@[adaptive]` fn name → recorded return
    /// scores, in call order. Read by `goal_run` (mirrors `axon-rt`'s store).
    provenance: RefCell<HashMap<String, Vec<f64>>>,
    /// Per-call i64-prefix input tuple, in lock-step with `provenance`. The
    /// vec collects every leading i64 arg the fn took (length = how many
    /// of the fn's first args were i64). Empty when none were. Read by
    /// `goal_best_input` (returns the first dim) and `goal_best_inputs`
    /// (returns the full tuple), and used by the multi-arg coordinate-
    /// descent hill-climb to seed the next sweep.
    provenance_inputs: RefCell<HashMap<String, Vec<Vec<i64>>>>,
    /// Per-call f64-prefix input tuple, mirror of `provenance_inputs` for
    /// `@[adaptive] fn(f64, …) -> f64`. Read by `goal_best_input_f64` /
    /// `goal_best_inputs_f64`. Lets the optimizer cover continuous-domain
    /// problems (linear-regression weights, control parameters, etc.)
    /// without forcing the user to discretize via integer indices.
    provenance_inputs_f64: RefCell<HashMap<String, Vec<Vec<f64>>>>,
    /// Current call-stack depth, bounded by `RECURSION_LIMIT` so runaway
    /// recursion fails with a catchable panic rather than overflowing the
    /// (large but finite) interpreter thread stack and aborting the process.
    call_depth: Cell<usize>,
}

/// Max interpreter call depth before a graceful "recursion limit" panic. The
/// debug-build `eval` frame is large (~128 KB/call), so a 1 GiB thread stack
/// overflows around ~8000 frames; this limit fires first, turning runaway
/// recursion into a catchable panic instead of a process-aborting overflow.
const RECURSION_LIMIT: usize = 6_000;

/// Decrements the call-depth counter when a `call_fn` frame unwinds (any path).
struct DepthGuard<'a>(&'a Cell<usize>);
impl Drop for DepthGuard<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get().saturating_sub(1));
    }
}

/// Run `f` on a thread with a large stack. The tree-walking interpreter uses a
/// lot of native stack per call, so an 8 MB main stack overflows at only a few
/// hundred frames; this lets reasonably deep recursion run, while the
/// `RECURSION_LIMIT` guard backstops truly runaway recursion with a clean panic.
fn on_deep_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(1024 * 1024 * 1024)
            .spawn_scoped(s, f)
            .expect("spawn interpreter thread")
            .join()
            .expect("interpreter thread panicked")
    })
}

/// Parse-and-run convenience: returns the process exit code.
pub fn run_program(program: &Program) -> i32 {
    on_deep_stack(|| run_program_inner(program))
}

fn run_program_inner(program: &Program) -> i32 {
    let mut interp = Interp::build(program);
    let outcome = interp.init_globals().and_then(|()| interp.run_main());
    match outcome {
        Ok(Value::Int(n)) => n as i32,
        Ok(_) => 0,
        Err(Flow::Exit(code)) => code,
        Err(Flow::Panic(msg)) => {
            let _ = std::io::stdout().flush();
            eprintln!("axon: panic: {msg}");
            101
        }
        // A stray return/break/continue escaping `main` — treat as clean exit.
        Err(_) => 0,
    }
}

/// Run a single zero-argument function (e.g. an `@[test]`) by name.
///
/// Returns `Ok(())` if it completed without panicking, or `Err(message)` on a
/// runtime panic / non-zero `exit`. Used by `axon test` to run tests in-process.
pub fn run_test_fn(program: &Program, name: &str) -> Result<(), String> {
    on_deep_stack(|| run_test_fn_inner(program, name))
}

fn run_test_fn_inner(program: &Program, name: &str) -> Result<(), String> {
    let mut interp = Interp::build(program);
    if let Err(f) = interp.init_globals() {
        return Err(flow_to_msg(f));
    }
    let Some(f) = interp.fns.get(name).copied() else {
        return Err(format!("no function `{name}`"));
    };
    match interp.call_fn(f, vec![]) {
        Ok(_) => Ok(()),
        Err(Flow::Panic(m)) => Err(m),
        Err(Flow::Exit(0)) => Ok(()),
        Err(Flow::Exit(n)) => Err(format!("exited with code {n}")),
        // A stray return/break/continue escaping the fn — treat as clean.
        Err(_) => Ok(()),
    }
}

fn flow_to_msg(f: Flow) -> String {
    match f {
        Flow::Panic(m) => m,
        Flow::Exit(n) => format!("exited with code {n}"),
        _ => "non-local control flow escaped the program".into(),
    }
}

impl<'p> Interp<'p> {
    pub fn build(program: &'p Program) -> Self {
        let mut fns = HashMap::new();
        let mut structs = HashMap::new();
        let mut enums = HashMap::new();
        let mut methods = HashMap::new();
        let mut global_defs = Vec::new();

        for item in &program.items {
            match item {
                Item::FnDef(f) => {
                    fns.insert(f.name.clone(), f);
                }
                Item::TypeDef(t) => {
                    structs.insert(t.name.clone(), t);
                }
                Item::EnumDef(e) => {
                    enums.insert(e.name.clone(), e);
                }
                Item::ImplBlock(ImplBlock { for_type, methods: ms, .. }) => {
                    let tn = type_name_of(for_type);
                    for m in ms {
                        methods.insert((tn.clone(), m.name.clone()), m);
                    }
                }
                Item::LetDef { name, value, .. } => {
                    global_defs.push((name.clone(), value.as_ref()));
                }
                _ => {}
            }
        }

        Interp {
            fns,
            structs,
            enums,
            methods,
            global_defs,
            globals: HashMap::new(),
            provenance: RefCell::new(HashMap::new()),
            provenance_inputs: RefCell::new(HashMap::new()),
            provenance_inputs_f64: RefCell::new(HashMap::new()),
            call_depth: Cell::new(0),
        }
    }

    /// Evaluate module-level constants in source order, so each may reference
    /// those defined before it. Populates [`Interp::globals`].
    fn init_globals(&mut self) -> Result<(), Flow> {
        if self.global_defs.is_empty() {
            return Ok(());
        }
        let defs = std::mem::take(&mut self.global_defs);
        let mut env = Env::new();
        for (name, expr) in &defs {
            let v = self.eval(expr, &mut env)?;
            env.define(name.clone(), v);
        }
        self.globals = env.snapshot();
        Ok(())
    }

    /// Run `main` with no arguments.
    fn run_main(&self) -> R {
        match self.fns.get("main") {
            Some(f) => self.call_fn(f, vec![]),
            None => panic("no `main` function"),
        }
    }

    // ── Function / closure calls ─────────────────────────────────────────────

    fn call_fn(&self, f: &FnDef, args: Vec<Value>) -> R {
        // Bound recursion: a graceful panic instead of a process-aborting stack
        // overflow on runaway/infinite recursion. `_guard` restores the depth on
        // any return path (including `?`).
        let depth = self.call_depth.get() + 1;
        if depth > RECURSION_LIMIT {
            return panic(format!(
                "recursion limit exceeded ({RECURSION_LIMIT}) in `{}` — infinite or excessively deep recursion?",
                f.name
            ));
        }
        self.call_depth.set(depth);
        let _guard = DepthGuard(&self.call_depth);

        if f.params.len() != args.len() {
            return panic(format!(
                "{}: expected {} args, got {}",
                f.name,
                f.params.len(),
                args.len()
            ));
        }
        // The leading i64 / f64 args (if any) form the goal-search input
        // tuple — recorded so goal_run can resume from the best prior probe
        // and multi-arg coordinate descent can seed each dim independently.
        // Two parallel collectors so an i64-prefix fn and an f64-prefix fn
        // both populate the right store; we choose the right one based on
        // the fn's signature in `run_goal`.
        let input_args: Vec<i64> = args
            .iter()
            .take_while(|v| matches!(v, Value::Int(_)))
            .map(|v| if let Value::Int(n) = v { *n } else { 0 })
            .collect();
        let input_args_f64: Vec<f64> = args
            .iter()
            .take_while(|v| matches!(v, Value::Float(_)))
            .map(|v| if let Value::Float(f) = v { *f } else { 0.0 })
            .collect();
        // First dim, for back-compat with the verify-panic enrichment that
        // reports a single "input N" suffix.
        let input_arg: Option<i64> = input_args.first().copied();
        let mut env = Env::new();
        for (p, a) in f.params.iter().zip(args) {
            env.define(p.name.clone(), a);
        }
        let result = match self.eval(&f.body, &mut env) {
            Ok(v) => v,
            Err(Flow::Return(v)) => v,
            Err(other) => return Err(other),
        };

        // `@[adaptive]`: provenance-log each call's numeric return so `goal_run`
        // can read it back (mirrors the codegen `@[adaptive]` prologue).
        if f.attrs.iter().any(|a| a.name == "adaptive") {
            if let Some(score) = numeric_score(&result) {
                self.provenance
                    .borrow_mut()
                    .entry(f.name.clone())
                    .or_default()
                    .push(score);
                self.provenance_inputs
                    .borrow_mut()
                    .entry(f.name.clone())
                    .or_default()
                    .push(input_args.clone());
                self.provenance_inputs_f64
                    .borrow_mut()
                    .entry(f.name.clone())
                    .or_default()
                    .push(input_args_f64.clone());
                // Also persist to the provenance JSONL (axon-rt's format) so
                // `axon trace`/observability tooling see interpreter runs.
                let payload = match &result {
                    Value::Int(n) => format!("ret_i64={n}"),
                    _ => format!("ret_f64={score}"),
                };
                append_provenance_jsonl(&f.name, &payload, score, input_arg);
            }
        }

        // `@[verify(predicate)]`: runtime gate. Two paths:
        //  - For `confidence OP K` / `value OP K` (the codegen-decodable
        //    shapes), do the comparison directly and emit a rich panic
        //    naming both fields and the search input. This matches what
        //    native codegen will emit when the codegen build completes.
        //  - For anything more complex (`&&`, `||`, multi-field, function
        //    calls in the predicate), bind `confidence` / `value` /
        //    `source_tag` into a fresh env and evaluate the predicate as
        //    a normal Expr. Lets the user write
        //    `@[verify(value > 0 && confidence >= 0.8)]` and have it
        //    enforced at runtime — closing ROADMAP §9.5 F6.
        if let Some(spec) = &f.verify {
            if let Value::Struct { name, fields } = &result {
                if name == "Uncertain" {
                    let decoded = crate::verify::decode_verify_predicate_with_ident(&spec.predicate);
                    let val_str = fields.get("value").map(display).unwrap_or_else(|| "?".into());
                    let conf_str = fields.get("confidence").map(display).unwrap_or_else(|| "?".into());
                    let input_str = input_arg.map(|n| format!(", input {n}")).unwrap_or_default();

                    if let Some((ident, op, bound)) = decoded {
                        // Simple shape: do the targeted, well-typed compare.
                        let observed: Option<f64> = match (ident.as_str(), fields.get(ident.as_str())) {
                            ("confidence", Some(Value::Float(c))) => Some(*c),
                            ("value", Some(Value::Int(n))) => Some(*n as f64),
                            ("value", Some(Value::Float(v))) => Some(*v),
                            _ => None,
                        };
                        if let Some(c) = observed {
                            if !cmp_f64(&op, c, bound) {
                                return Err(Flow::Panic(format!(
                                    "verify failed in `{}`: {} {} {} {} is false \
                                     (value {}, confidence {}{})",
                                    f.name,
                                    ident,
                                    c,
                                    crate::verify::binop_to_verify_str(&op),
                                    bound,
                                    val_str,
                                    conf_str,
                                    input_str,
                                )));
                            }
                        }
                    } else {
                        // Composite predicate: evaluate as a normal Expr with
                        // `value`, `confidence`, `source_tag` in scope. Any
                        // boolean expression Axon understands is accepted —
                        // `&&`, `||`, comparisons, function calls, you name it.
                        let mut pred_env = Env::new();
                        if let Some(v) = fields.get("value") {
                            pred_env.define("value".into(), v.clone());
                        }
                        if let Some(c) = fields.get("confidence") {
                            pred_env.define("confidence".into(), c.clone());
                        }
                        if let Some(s) = fields.get("source_tag") {
                            pred_env.define("source_tag".into(), s.clone());
                        }
                        let outcome = self.eval(&spec.predicate, &mut pred_env)?;
                        if let Value::Bool(false) = outcome {
                            return Err(Flow::Panic(format!(
                                "verify failed in `{}`: composite predicate did not hold \
                                 (value {}, confidence {}{})",
                                f.name,
                                val_str,
                                conf_str,
                                input_str,
                            )));
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    fn call_closure(&self, c: Value, args: Vec<Value>) -> R {
        let Value::Closure { params, body, captured } = c else {
            return panic(format!("value of type {} is not callable", c.type_name()));
        };
        if params.len() != args.len() {
            return panic(format!(
                "lambda: expected {} args, got {}",
                params.len(),
                args.len()
            ));
        }
        let mut env = Env::new();
        // Base scope = captured bindings; a fresh scope holds the parameters.
        *env.scopes.last_mut().unwrap() = captured;
        env.push();
        for (p, a) in params.iter().zip(args) {
            env.define(p.clone(), a);
        }
        match self.eval(&body, &mut env) {
            Ok(v) => Ok(v),
            Err(Flow::Return(v)) => Ok(v),
            Err(other) => Err(other),
        }
    }

    // ── goal_run: hill-climb / retrospective best-observed ───────────────────

    /// `goal_run(name, target, max_evals)`. Live hill-climb when `name` is an
    /// `@[adaptive] fn(i64) -> i64` in the program; otherwise a retrospective
    /// best-observed lookup over the provenance store. Mirrors `axon-rt`'s
    /// `goal.rs` semantics.
    /// Warm-start counterpart of `run_goal`. Reads the best prior probe from
    /// in-memory provenance and seeds the multi-arg hill climb there;
    /// single-arg paths use the existing on-disk continuation hook unchanged.
    /// Falls through to fresh start (origin seed) when no prior best exists,
    /// so calling cold is a no-op.
    fn run_goal_warm(&self, name: &str, target: f64, max_evals: i64) -> Result<f64, Flow> {
        if let Some(f) = self.fns.get(name) {
            let f = *f;
            let is_adaptive = f.attrs.iter().any(|a| a.name == "adaptive");
            let all_i64_params = !f.params.is_empty()
                && f.params.iter().all(|p| is_i64_type(&p.ty));
            let all_f64_params = !f.params.is_empty()
                && f.params.iter().all(|p| is_f64_type(&p.ty));
            let i64_ret = f.return_type.as_ref().map(is_i64_type).unwrap_or(false);
            let f64_ret = f.return_type.as_ref().map(is_f64_type).unwrap_or(false);
            if is_adaptive {
                if all_i64_params && i64_ret {
                    if f.params.len() == 1 {
                        // Single-arg: already has the on-disk continuation hook.
                        return self.hill_climb_i64(f, target, max_evals);
                    }
                    // Multi-arg: seed from in-memory best prior tuple.
                    let seed = self.best_input_index(name, target).and_then(|idx| {
                        self.provenance_inputs
                            .borrow()
                            .get(name)
                            .and_then(|v| v.get(idx).cloned())
                    });
                    return self.hill_climb_multi_i64_from(f, target, max_evals, seed);
                }
                if all_f64_params && f64_ret {
                    let seed = self.best_input_index(name, target).and_then(|idx| {
                        self.provenance_inputs_f64
                            .borrow()
                            .get(name)
                            .and_then(|v| v.get(idx).cloned())
                    });
                    return self.hill_climb_multi_f64_from(f, target, max_evals, seed);
                }
            }
        }
        Ok(self.best_observed(name, target, max_evals))
    }

    /// Random-search strategy for an `@[adaptive] fn(i64, …) -> i64`.
    /// Samples `n_samples` random i64 tuples uniformly in `[lo, hi)`
    /// (per-dim independent) and scores each. Returns the best score
    /// (closest to target). Provides a baseline against `goal_run`'s
    /// hill climb — useful for multi-modal objectives where the
    /// gradient strategy gets stuck in local optima, and for
    /// "is the optimizer actually doing anything?" sanity checks.
    /// Each call flows through `call_fn`, so provenance accumulates
    /// just like the hill-climb path; `goal_best_input` / `_inputs`
    /// can read the winner back.
    fn run_goal_random(
        &self,
        name: &str,
        target: f64,
        n_samples: i64,
        lo: i64,
        hi: i64,
    ) -> Result<f64, Flow> {
        if hi <= lo {
            return panic(format!(
                "goal_run_random: hi ({hi}) must be greater than lo ({lo})"
            ));
        }
        let f = match self.fns.get(name) {
            Some(f) => *f,
            None => return Ok(self.best_observed(name, target, n_samples)),
        };
        let is_adaptive = f.attrs.iter().any(|a| a.name == "adaptive");
        let all_i64_params = !f.params.is_empty()
            && f.params.iter().all(|p| is_i64_type(&p.ty));
        let i64_ret = f.return_type.as_ref().map(is_i64_type).unwrap_or(false);
        if !is_adaptive || !all_i64_params || !i64_ret {
            return Ok(self.best_observed(name, target, n_samples));
        }
        let n_dims = f.params.len();
        // Start "unset" — first probe wins regardless of its distance,
        // then every subsequent probe must beat it. Initializing from
        // `best_observed` is wrong here because that returns `target`
        // when no provenance exists, locking `best_dist` at 0 and
        // preventing every later probe from being accepted.
        let mut best_score: f64 = f64::NAN;
        let mut best_dist: f64 = f64::INFINITY;
        let mut i: i64 = 0;
        while i < n_samples {
            // Uniform per-dim in `[lo, hi)` via the same `next_rand_u64`
            // helper the `random_i64` builtin uses — keeps RNG semantics
            // consistent across calls within one run.
            let mut probe: Vec<Value> = Vec::with_capacity(n_dims);
            let range = (hi as i128 - lo as i128) as u128;
            for _ in 0..n_dims {
                let v = lo + (next_rand_u64() as u128 % range.max(1)) as i64;
                probe.push(Value::Int(v));
            }
            let score = match self.call_fn(f, probe)? {
                Value::Int(n) => n as f64,
                Value::Float(v) => v,
                other => return panic(format!(
                    "@[adaptive] fn `{}` must return a number, got {}",
                    f.name,
                    other.type_name()
                )),
            };
            let d = (score - target).abs();
            if d < best_dist {
                best_dist = d;
                best_score = score;
                if best_dist <= f64::EPSILON {
                    return Ok(best_score);
                }
            }
            i += 1;
        }
        // If no probes ran (n_samples <= 0), fall back to "target" so the
        // caller's contract ("return the best") still makes sense.
        if best_score.is_nan() {
            best_score = target;
        }
        Ok(best_score)
    }

    /// Multi-start hill climb. Picks `n_starts` random starting points
    /// uniformly in `[lo, hi)` (per-dim) and runs the existing
    /// coordinate-descent hill-climb (with Powell joint step) from each
    /// with `evals_per_start` budget. Returns the best score across all
    /// starts. Standard recipe for escaping local optima on multi-modal
    /// objectives — keeps gradient-style refinement once a basin is
    /// found while still sampling broadly. Provenance accumulates as a
    /// side effect (so goal_best_input reads the winning probe back).
    fn run_goal_multistart(
        &self,
        name: &str,
        target: f64,
        n_starts: i64,
        evals_per_start: i64,
        lo: i64,
        hi: i64,
    ) -> Result<f64, Flow> {
        if hi <= lo {
            return panic(format!(
                "goal_run_multistart: hi ({hi}) must be greater than lo ({lo})"
            ));
        }
        let f = match self.fns.get(name) {
            Some(f) => *f,
            None => return Ok(self.best_observed(name, target, 0)),
        };
        let is_adaptive = f.attrs.iter().any(|a| a.name == "adaptive");
        let all_i64_params = !f.params.is_empty()
            && f.params.iter().all(|p| is_i64_type(&p.ty));
        let i64_ret = f.return_type.as_ref().map(is_i64_type).unwrap_or(false);
        if !is_adaptive || !all_i64_params || !i64_ret {
            return Ok(self.best_observed(name, target, 0));
        }
        let n_dims = f.params.len();
        let mut best_score: f64 = f64::NAN;
        let mut best_dist: f64 = f64::INFINITY;
        let range = (hi as i128 - lo as i128) as u128;
        let mut s: i64 = 0;
        while s < n_starts {
            let start: Vec<i64> = (0..n_dims)
                .map(|_| lo + (next_rand_u64() as u128 % range.max(1)) as i64)
                .collect();
            let score = if n_dims == 1 {
                // Single-arg: bypass the multi-i64 path's per-dim wiring
                // (which expects Vec<i64>) and call the 1-D climber
                // directly, but its API doesn't take a start point —
                // use multi-i64_from with a 1-elem vec instead.
                self.hill_climb_multi_i64_from(f, target, evals_per_start, Some(start))?
            } else {
                self.hill_climb_multi_i64_from(f, target, evals_per_start, Some(start))?
            };
            let d = (score - target).abs();
            if d < best_dist {
                best_dist = d;
                best_score = score;
                if best_dist <= f64::EPSILON {
                    return Ok(best_score);
                }
            }
            s += 1;
        }
        if best_score.is_nan() {
            best_score = target;
        }
        Ok(best_score)
    }

    fn run_goal(&self, name: &str, target: f64, max_evals: i64) -> Result<f64, Flow> {
        if let Some(f) = self.fns.get(name) {
            let f = *f;
            let is_adaptive = f.attrs.iter().any(|a| a.name == "adaptive");
            let all_i64_params = !f.params.is_empty()
                && f.params.iter().all(|p| is_i64_type(&p.ty));
            let all_f64_params = !f.params.is_empty()
                && f.params.iter().all(|p| is_f64_type(&p.ty));
            let i64_ret = f.return_type.as_ref().map(is_i64_type).unwrap_or(false);
            let f64_ret = f.return_type.as_ref().map(is_f64_type).unwrap_or(false);
            if is_adaptive {
                if all_i64_params && i64_ret {
                    return if f.params.len() == 1 {
                        self.hill_climb_i64(f, target, max_evals)
                    } else {
                        self.hill_climb_multi_i64(f, target, max_evals)
                    };
                }
                if all_f64_params && f64_ret {
                    return self.hill_climb_multi_f64(f, target, max_evals);
                }
            }
        }
        Ok(self.best_observed(name, target, max_evals))
    }

    /// Hill-climb an `@[adaptive] fn(i64) -> i64` toward `target`, accepting any
    /// improvement and halving the step on a stall. Direction-agnostic: returns
    /// the observed score closest to `target`. Each callback flows through
    /// [`Interp::call_fn`], so the provenance store accumulates as a side effect.
    fn hill_climb_i64(&self, f: &FnDef, target: f64, max_evals: i64) -> Result<f64, Flow> {
        let eval_at = |x: i64| -> Result<f64, Flow> {
            match self.call_fn(f, vec![Value::Int(x)])? {
                Value::Int(n) => Ok(n as f64),
                Value::Float(v) => Ok(v),
                other => panic(format!(
                    "@[adaptive] fn `{}` must return a number, got {}",
                    f.name,
                    other.type_name()
                )),
            }
        };

        // Resume from the best prior input across runs when continuation is
        // enabled (`AXON_GOAL_CONTINUE`); otherwise start fresh at 0.
        let mut cur_input: i64 = if goal_continue_enabled() {
            read_best_input(&f.name, target).unwrap_or(0)
        } else {
            0
        };
        let initial = eval_at(cur_input)?;
        let mut best_score = initial;
        let mut best_dist = (initial - target).abs();
        let mut best_input = cur_input;
        let mut evals: i64 = 1;
        let unlimited = max_evals <= 0;
        // Early exit if the very first probe already hit target — no point
        // burning the rest of the budget on a perfect score (and downstream
        // tests / observability are cleaner when the trace doesn't include
        // redundant tail evals).
        if best_dist <= f64::EPSILON {
            return Ok(best_score);
        }
        // Coarse-then-fine: when the starting probe is small (e.g. 0), the
        // old `step = max(1, |x|/4)` formula locked us into a unit-step walk
        // and 50 evals never escaped the local neighborhood. Seed wide and
        // let the halving phase narrow toward the optimum.
        //
        // Two modes:
        //  - Fresh start (cur_input == 0 and no prior best on disk): seed
        //    `step ≈ max_evals * 4` so the first probes can leap across
        //    the whole plausible range and the halving cascade acts as a
        //    binary search toward the peak.
        //  - Continuation / nonzero start: assume the input is already in a
        //    good neighborhood, use the narrow `|x|/4` seed (with a floor
        //    of 1) so we fine-tune rather than overshoot — preserves the
        //    cross-run continuation semantics tested by `learn-goal.md`.
        let mut step: i64 = if cur_input == 0 {
            if unlimited { 4096 } else { std::cmp::max(16, max_evals.saturating_mul(4)) }
        } else {
            std::cmp::max(1, (cur_input.unsigned_abs() as i64) / 4)
        };

        while step >= 1 {
            if !unlimited && evals >= max_evals {
                break;
            }
            let mut improved = false;

            let up = cur_input.saturating_add(step);
            let up_score = eval_at(up)?;
            evals += 1;
            if (up_score - target).abs() < best_dist {
                best_dist = (up_score - target).abs();
                best_score = up_score;
                best_input = up;
                improved = true;
                // Hit target exactly — stop now so subsequent observability
                // (goal_history, goal_best_input) reflects a tight trace.
                if best_dist <= f64::EPSILON {
                    return Ok(best_score);
                }
            }
            if !unlimited && evals >= max_evals {
                break;
            }

            let dn = cur_input.saturating_sub(step);
            let dn_score = eval_at(dn)?;
            evals += 1;
            if (dn_score - target).abs() < best_dist {
                best_dist = (dn_score - target).abs();
                best_score = dn_score;
                best_input = dn;
                improved = true;
                if best_dist <= f64::EPSILON {
                    return Ok(best_score);
                }
            }

            if improved {
                cur_input = best_input;
            } else {
                step /= 2;
            }
        }
        Ok(best_score)
    }

    /// Coordinate-descent hill climb for an `@[adaptive] fn(i64, i64, ...) -> i64`.
    /// Cycles through each dim, halving-step searching that dim while holding
    /// the others fixed, then repeats the sweep until either no dim improves
    /// (its halving cascade bottomed out) or the budget is exhausted.
    /// Direction-agnostic: returns the observed score closest to `target`.
    /// Each callback flows through `Interp::call_fn`, so provenance (scores +
    /// full input tuples) accumulates as a side effect.
    fn hill_climb_multi_i64(
        &self,
        f: &FnDef,
        target: f64,
        max_evals: i64,
    ) -> Result<f64, Flow> {
        self.hill_climb_multi_i64_from(f, target, max_evals, None)
    }

    fn hill_climb_multi_i64_from(
        &self,
        f: &FnDef,
        target: f64,
        max_evals: i64,
        start: Option<Vec<i64>>,
    ) -> Result<f64, Flow> {
        let n_dims = f.params.len();
        let unlimited = max_evals <= 0;
        // `start = None` means fresh: cur = [0; n_dims]. `start = Some(v)`
        // (used by `goal_continue`) seeds at a known-good prior probe.
        // Length-mismatches fall back to zero so a stale store can't crash
        // the optimizer.
        let mut cur: Vec<i64> = match start {
            Some(v) if v.len() == n_dims => v,
            _ => vec![0; n_dims],
        };
        let eval_at = |xs: &[i64]| -> Result<f64, Flow> {
            let args = xs.iter().map(|&x| Value::Int(x)).collect();
            match self.call_fn(f, args)? {
                Value::Int(n) => Ok(n as f64),
                Value::Float(v) => Ok(v),
                other => panic(format!(
                    "@[adaptive] fn `{}` must return a number, got {}",
                    f.name,
                    other.type_name()
                )),
            }
        };

        let mut best_score = eval_at(&cur)?;
        let mut best_dist = (best_score - target).abs();
        let mut evals: i64 = 1;
        if best_dist <= f64::EPSILON {
            return Ok(best_score);
        }

        // Per-dim step seeding. Mirrors the 1-D formula: each dim gets a
        // wide first probe so it can sweep an interval before the halving
        // cascade narrows. We divide the budget across dims so a single
        // sweep stays inside the user's eval cap.
        let per_dim_budget = if unlimited { 0 } else { std::cmp::max(4, max_evals / (n_dims as i64).max(1)) };
        let seed_step: i64 = if unlimited { 4096 } else { std::cmp::max(16, per_dim_budget.saturating_mul(4)) };

        let mut steps: Vec<i64> = vec![seed_step; n_dims];
        // Per-dim sweep cap rotates dims fairly so no single dim monopolizes
        // the budget. Less critical here than in the f64 path (i64 halving
        // bottoms out in ~log2(seed_step) ≈ 12 steps, well inside a fair
        // share), but keeps the algorithm symmetric and forward-compatible.
        let per_dim_sweep_cap = if unlimited {
            i64::MAX
        } else {
            std::cmp::max(4, per_dim_budget)
        };

        // Sweep until no dim improves or budget hits.
        loop {
            let mut any_improvement = false;
            let cur_at_sweep_start = cur.clone();
            for d in 0..n_dims {
                if !unlimited && evals >= max_evals {
                    return Ok(best_score);
                }
                let dim_evals_at_start = evals;
                while steps[d] >= 1 {
                    if !unlimited && evals >= max_evals {
                        return Ok(best_score);
                    }
                    if !unlimited && evals - dim_evals_at_start >= per_dim_sweep_cap {
                        break;
                    }
                    let mut improved = false;

                    let mut probe = cur.clone();
                    probe[d] = cur[d].saturating_add(steps[d]);
                    let up_score = eval_at(&probe)?;
                    evals += 1;
                    if (up_score - target).abs() < best_dist {
                        best_dist = (up_score - target).abs();
                        best_score = up_score;
                        cur[d] = probe[d];
                        improved = true;
                        any_improvement = true;
                        if best_dist <= f64::EPSILON {
                            return Ok(best_score);
                        }
                    }
                    if !unlimited && evals >= max_evals {
                        return Ok(best_score);
                    }

                    let mut probe = cur.clone();
                    probe[d] = cur[d].saturating_sub(steps[d]);
                    let dn_score = eval_at(&probe)?;
                    evals += 1;
                    if (dn_score - target).abs() < best_dist {
                        best_dist = (dn_score - target).abs();
                        best_score = dn_score;
                        cur[d] = probe[d];
                        improved = true;
                        any_improvement = true;
                        if best_dist <= f64::EPSILON {
                            return Ok(best_score);
                        }
                    }

                    if !improved {
                        steps[d] /= 2;
                    }
                }
            }
            if !any_improvement {
                return Ok(best_score);
            }

            // Powell's-method joint-direction step. Same heuristic as the
            // f64 path: extrapolate along `delta = cur - cur_at_sweep_start`
            // with k = 1, 2, 4, … while the score keeps improving. On a
            // multi-arg objective where the per-dim sweeps each found
            // improvements in correlated directions, the joint step can
            // jump straight to a good neighbourhood that cyclic CD would
            // crawl toward across many sweeps.
            let mut delta = vec![0_i64; n_dims];
            let mut delta_nonzero = false;
            for d in 0..n_dims {
                delta[d] = cur[d].saturating_sub(cur_at_sweep_start[d]);
                if delta[d] != 0 {
                    delta_nonzero = true;
                }
            }
            if delta_nonzero {
                let mut k: i64 = 1;
                loop {
                    if !unlimited && evals >= max_evals {
                        return Ok(best_score);
                    }
                    let probe: Vec<i64> = (0..n_dims)
                        .map(|d| cur[d].saturating_add(k.saturating_mul(delta[d])))
                        .collect();
                    let probe_score = eval_at(&probe)?;
                    evals += 1;
                    let probe_dist = (probe_score - target).abs();
                    if probe_dist < best_dist {
                        best_dist = probe_dist;
                        best_score = probe_score;
                        cur = probe;
                        if best_dist <= f64::EPSILON {
                            return Ok(best_score);
                        }
                        k = k.saturating_mul(2);
                        if k > 1 << 30 {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }

            // Re-arm each dim's step for another sweep — a higher-dim move
            // may have opened a new gradient in dim 0.
            for s in steps.iter_mut() {
                *s = seed_step;
            }
        }
    }

    /// Coordinate-descent hill climb for an `@[adaptive] fn(f64, …) -> f64`.
    /// Mirror of `hill_climb_multi_i64` over the continuous domain: per-dim
    /// halving step starting wide, cycle through dims until a full sweep
    /// produces no improvement (each dim's step bottomed out below the
    /// resolution). The resolution floor is `1e-9` — tight enough for
    /// learned weights, well above f64 precision noise.
    fn hill_climb_multi_f64(
        &self,
        f: &FnDef,
        target: f64,
        max_evals: i64,
    ) -> Result<f64, Flow> {
        self.hill_climb_multi_f64_from(f, target, max_evals, None)
    }

    fn hill_climb_multi_f64_from(
        &self,
        f: &FnDef,
        target: f64,
        max_evals: i64,
        start: Option<Vec<f64>>,
    ) -> Result<f64, Flow> {
        let n_dims = f.params.len();
        let unlimited = max_evals <= 0;
        let mut cur: Vec<f64> = match start {
            Some(v) if v.len() == n_dims => v,
            _ => vec![0.0; n_dims],
        };
        let eval_at = |xs: &[f64]| -> Result<f64, Flow> {
            let args = xs.iter().map(|&x| Value::Float(x)).collect();
            match self.call_fn(f, args)? {
                Value::Float(v) => Ok(v),
                Value::Int(n) => Ok(n as f64),
                other => panic(format!(
                    "@[adaptive] fn `{}` must return a number, got {}",
                    f.name,
                    other.type_name()
                )),
            }
        };

        let mut best_score = eval_at(&cur)?;
        let mut best_dist = (best_score - target).abs();
        let mut evals: i64 = 1;
        if best_dist <= f64::EPSILON {
            return Ok(best_score);
        }

        // Seed step: wide enough to leap across a few-hundred-unit window
        // and let halving zero in. Mirrors the i64 path but scales the
        // budget partition by dim count.
        let per_dim_budget = if unlimited { 0 } else { std::cmp::max(4, max_evals / (n_dims as i64).max(1)) };
        let seed_step: f64 = if unlimited { 1024.0 } else { (per_dim_budget as f64 * 4.0).max(16.0) };
        let resolution: f64 = 1e-9;

        let mut steps: Vec<f64> = vec![seed_step; n_dims];

        // Cap each dim's evals per sweep so no single dim monopolizes the
        // budget. Without this, the inner halving cascade on dim 0 (37+
        // halvings × 2 evals = ~74 evals to fully bottom out at the f64
        // resolution floor) could eat a small total budget before dim 1
        // gets a single probe. `per_dim_sweep_cap` rotates dims fairly;
        // multiple sweeps still let any single dim fully converge, just
        // not in one greedy pass.
        let per_dim_sweep_cap = if unlimited {
            i64::MAX
        } else {
            std::cmp::max(4, per_dim_budget)
        };

        loop {
            let mut any_improvement = false;
            let cur_at_sweep_start = cur.clone();
            for d in 0..n_dims {
                if !unlimited && evals >= max_evals {
                    return Ok(best_score);
                }
                let dim_evals_at_start = evals;
                while steps[d] >= resolution {
                    if !unlimited && evals >= max_evals {
                        return Ok(best_score);
                    }
                    if !unlimited && evals - dim_evals_at_start >= per_dim_sweep_cap {
                        break;
                    }
                    let mut improved = false;

                    let mut probe = cur.clone();
                    probe[d] = cur[d] + steps[d];
                    let up_score = eval_at(&probe)?;
                    evals += 1;
                    if (up_score - target).abs() < best_dist {
                        best_dist = (up_score - target).abs();
                        best_score = up_score;
                        cur[d] = probe[d];
                        improved = true;
                        any_improvement = true;
                        if best_dist <= f64::EPSILON {
                            return Ok(best_score);
                        }
                    }
                    if !unlimited && evals >= max_evals {
                        return Ok(best_score);
                    }

                    let mut probe = cur.clone();
                    probe[d] = cur[d] - steps[d];
                    let dn_score = eval_at(&probe)?;
                    evals += 1;
                    if (dn_score - target).abs() < best_dist {
                        best_dist = (dn_score - target).abs();
                        best_score = dn_score;
                        cur[d] = probe[d];
                        improved = true;
                        any_improvement = true;
                        if best_dist <= f64::EPSILON {
                            return Ok(best_score);
                        }
                    }

                    if !improved {
                        steps[d] *= 0.5;
                    }
                }
            }
            if !any_improvement {
                return Ok(best_score);
            }

            // Powell's-method-style joint-direction step. After a sweep
            // that improved each dim individually, the net displacement
            // `delta = cur - cur_at_sweep_start` often points toward the
            // joint optimum for correlated dims (slope/intercept in a
            // linear-regression objective being the classic case). Probe
            // along that direction with k = 1, 2, 4, … while score keeps
            // improving — a geometric line search. Cuts the constant-
            // factor convergence rate of cyclic CD on quadratic objectives.
            let mut delta = vec![0.0_f64; n_dims];
            let mut delta_norm_sq = 0.0_f64;
            for d in 0..n_dims {
                delta[d] = cur[d] - cur_at_sweep_start[d];
                delta_norm_sq += delta[d] * delta[d];
            }
            if delta_norm_sq > resolution * resolution {
                let mut k: f64 = 1.0;
                loop {
                    if !unlimited && evals >= max_evals {
                        return Ok(best_score);
                    }
                    let probe: Vec<f64> = (0..n_dims)
                        .map(|d| cur[d] + k * delta[d])
                        .collect();
                    let probe_score = eval_at(&probe)?;
                    evals += 1;
                    let probe_dist = (probe_score - target).abs();
                    if probe_dist < best_dist {
                        best_dist = probe_dist;
                        best_score = probe_score;
                        cur = probe;
                        if best_dist <= f64::EPSILON {
                            return Ok(best_score);
                        }
                        k *= 2.0;
                        if k > 1e6 {
                            break;
                        }
                    } else {
                        break;
                    }
                }
            }

            for s in steps.iter_mut() {
                *s = seed_step;
            }
        }
    }

    /// Closest recorded score to `target` for `name` (or `target` if none).
    /// `max_evals > 0` keeps only the most recent N records.
    fn best_observed(&self, name: &str, target: f64, max_evals: i64) -> f64 {
        if name.is_empty() {
            return target;
        }
        let store = self.provenance.borrow();
        let Some(scores) = store.get(name).filter(|s| !s.is_empty()) else {
            return target;
        };
        let slice: &[f64] = if max_evals > 0 && (max_evals as usize) < scores.len() {
            &scores[scores.len() - max_evals as usize..]
        } else {
            &scores[..]
        };
        let mut best = slice[0];
        let mut best_dist = (best - target).abs();
        for &s in &slice[1..] {
            let d = (s - target).abs();
            if d < best_dist {
                best = s;
                best_dist = d;
            }
        }
        best
    }

    /// Full `(input, score)` trace for an `@[adaptive]` fn. Walks the
    /// in-memory provenance store in call order. Each entry's input is the
    /// FIRST i64 dim — multi-arg fns expose all dims via `goal_best_inputs`
    /// instead. Empty when nothing was recorded or the fn had no i64 args.
    fn history(&self, name: &str) -> Value {
        let mut out: Vec<Value> = Vec::new();
        if name.is_empty() {
            return Value::Array(out);
        }
        let scores_store = self.provenance.borrow();
        let inputs_store = self.provenance_inputs.borrow();
        if let (Some(scores), Some(inputs)) = (scores_store.get(name), inputs_store.get(name)) {
            let n = scores.len().min(inputs.len());
            out.reserve(n);
            for i in 0..n {
                if let Some(&first) = inputs[i].first() {
                    out.push(Value::Tuple(vec![
                        Value::Int(first),
                        Value::Float(scores[i]),
                    ]));
                }
            }
        }
        Value::Array(out)
    }

    /// Drop the recorded `(input, score)` history for an `@[adaptive]` fn so
    /// a follow-up experiment starts from a clean slate. Returns the number
    /// of records evicted (0 when `name` was absent or already empty).
    fn clear(&self, name: &str) -> i64 {
        if name.is_empty() {
            return 0;
        }
        let evicted = self
            .provenance
            .borrow_mut()
            .remove(name)
            .map(|v| v.len() as i64)
            .unwrap_or(0);
        self.provenance_inputs.borrow_mut().remove(name);
        evicted
    }

    /// Index of the provenance entry whose score is closest to `target`,
    /// among entries with at least one recorded input (i64 or f64). None
    /// when nothing was recorded for `name`. The "has at least one input"
    /// check accepts either store so f64-only fns aren't filtered out.
    fn best_input_index(&self, name: &str, target: f64) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        let scores_store = self.provenance.borrow();
        let inputs_store = self.provenance_inputs.borrow();
        let inputs_f64_store = self.provenance_inputs_f64.borrow();
        let scores = scores_store.get(name).filter(|s| !s.is_empty())?;
        let n = scores.len();
        let mut best_idx: Option<usize> = None;
        let mut best_dist = f64::INFINITY;
        for (i, &score) in scores.iter().enumerate().take(n) {
            let has_i64 = inputs_store
                .get(name)
                .and_then(|v| v.get(i))
                .is_some_and(|t| !t.is_empty());
            let has_f64 = inputs_f64_store
                .get(name)
                .and_then(|v| v.get(i))
                .is_some_and(|t| !t.is_empty());
            if has_i64 || has_f64 {
                let d = (score - target).abs();
                if d < best_dist {
                    best_dist = d;
                    best_idx = Some(i);
                }
            }
        }
        best_idx
    }

    /// Input that produced the score closest to `target` for an `@[adaptive]`
    /// fn. Returns the FIRST i64 dim — for multi-arg fns, use
    /// `goal_best_inputs` for the full tuple. Falls back to 0 when no inputs
    /// were recorded (the fn was never invoked, or it had no i64 args).
    fn best_input(&self, name: &str, target: f64) -> i64 {
        self.best_input_index(name, target)
            .and_then(|i| self.provenance_inputs.borrow().get(name)?.get(i)?.first().copied())
            .unwrap_or(0)
    }

    /// All i64 input dims that produced the score closest to `target` for an
    /// `@[adaptive]` fn. Returns an empty slice when nothing was recorded.
    fn best_inputs(&self, name: &str, target: f64) -> Value {
        let Some(idx) = self.best_input_index(name, target) else {
            return Value::Array(Vec::new());
        };
        let inputs_store = self.provenance_inputs.borrow();
        let dims = inputs_store
            .get(name)
            .and_then(|v| v.get(idx))
            .cloned()
            .unwrap_or_default();
        Value::Array(dims.into_iter().map(Value::Int).collect())
    }

    /// f64-flavored counterpart of `best_inputs`: returns the f64-prefix
    /// input tuple of the entry whose score is closest to `target`.
    fn best_inputs_f64(&self, name: &str, target: f64) -> Value {
        let Some(idx) = self.best_input_index(name, target) else {
            return Value::Array(Vec::new());
        };
        let inputs_store = self.provenance_inputs_f64.borrow();
        let dims = inputs_store
            .get(name)
            .and_then(|v| v.get(idx))
            .cloned()
            .unwrap_or_default();
        Value::Array(dims.into_iter().map(Value::Float).collect())
    }

    /// Flatten a place expression (`base.f[i].g …`) into the root variable name
    /// and a base-to-leaf list of steps, evaluating any index expressions now
    /// (so the later mutable walk holds no other borrow of `env`).
    fn flatten_place(&self, place: &Expr, env: &mut Env) -> Result<(String, Vec<PlaceStep>), Flow> {
        let mut steps = Vec::new();
        let mut cur = place;
        let base = loop {
            match cur {
                Expr::Ident(name) => break name.clone(),
                Expr::FieldAccess { receiver, field } => {
                    steps.push(PlaceStep::Field(field.clone()));
                    cur = receiver.as_ref();
                }
                Expr::Index { receiver, index } => {
                    let idx = as_int(&self.eval(index, env)?)?;
                    if idx < 0 {
                        return Err(Flow::Panic(format!("negative index {idx}")));
                    }
                    steps.push(PlaceStep::Index(idx as usize));
                    cur = receiver.as_ref();
                }
                _ => return Err(Flow::Panic("invalid assignment target".into())),
            }
        };
        steps.reverse();
        Ok((base, steps))
    }

    // ── Core evaluator ───────────────────────────────────────────────────────

    fn eval(&self, expr: &Expr, env: &mut Env) -> R {
        match expr {
            Expr::Literal(lit) => Ok(lit_to_val(lit)),

            Expr::Ident(name) => {
                if let Some(v) = env.get(name) {
                    Ok(v.clone())
                } else if let Some(v) = self.globals.get(name) {
                    Ok(v.clone())
                } else {
                    panic(format!("undefined identifier `{name}`"))
                }
            }

            Expr::Block(stmts) => self.eval_block(stmts, env),

            Expr::Let { name, value, .. }
            | Expr::Own { name, value, .. }
            | Expr::RefBind { name, value, .. } => {
                let v = self.eval(value, env)?;
                env.define(name.clone(), v);
                Ok(Value::Unit)
            }

            Expr::Assign { name, value } => {
                let v = self.eval(value, env)?;
                if env.assign(name, v) {
                    Ok(Value::Unit)
                } else {
                    panic(format!("assignment to undefined variable `{name}`"))
                }
            }

            // Place assignment: `<place> = v`, where `place` is a (possibly
            // nested) chain of index / field accesses rooted at a variable —
            // e.g. `xs[i] = v`, `s.field = v`, `grid[i][j] = v`, `cfg.row[i] = v`.
            // Phase 1 flattens the place to (base ident, steps), evaluating index
            // expressions; phase 2 walks the binding mutably and sets the leaf.
            Expr::AssignTo { place, value } => {
                let v = self.eval(value, env)?;
                let (base, steps) = self.flatten_place(place, env)?;
                let mut slot = env
                    .get_mut(&base)
                    .ok_or_else(|| Flow::Panic(format!("assignment to undefined variable `{base}`")))?;
                let (last, prefix) = steps.split_last().ok_or_else(|| Flow::Panic("invalid assignment target".into()))?;
                for step in prefix {
                    slot = match (step, slot) {
                        (PlaceStep::Field(f), Value::Struct { fields, .. }) => fields
                            .get_mut(f)
                            .ok_or_else(|| Flow::Panic(format!("no field `{f}`")))?,
                        (PlaceStep::Index(i), Value::Array(items)) => {
                            let n = items.len();
                            items
                                .get_mut(*i)
                                .ok_or_else(|| Flow::Panic(format!("index {i} out of bounds (len {n})")))?
                        }
                        (_, other) => {
                            return panic(format!("cannot index/field-assign into {}", other.type_name()));
                        }
                    };
                }
                match (last, slot) {
                    (PlaceStep::Field(f), Value::Struct { fields, .. }) => {
                        fields.insert(f.clone(), v);
                    }
                    (PlaceStep::Index(i), Value::Array(items)) => {
                        if *i >= items.len() {
                            return panic(format!("index {i} out of bounds (len {})", items.len()));
                        }
                        items[*i] = v;
                    }
                    (_, other) => {
                        return panic(format!("cannot index/field-assign into {}", other.type_name()));
                    }
                }
                Ok(Value::Unit)
            }

            Expr::BinOp { op, left, right } => self.eval_binop(op, left, right, env),

            Expr::UnaryOp { op, operand } => {
                let v = self.eval(operand, env)?;
                eval_unary(op, v)
            }

            Expr::If { cond, then, else_ } => match self.eval(cond, env)? {
                Value::Bool(true) => self.eval(then, env),
                Value::Bool(false) => match else_ {
                    Some(e) => self.eval(e, env),
                    None => Ok(Value::Unit),
                },
                other => panic(format!("if condition must be bool, got {}", other.type_name())),
            },

            Expr::Match { subject, arms } => {
                let v = self.eval(subject, env)?;
                for arm in arms {
                    env.push();
                    if self.match_pattern(&arm.pattern, &v, env)? {
                        if let Some(guard) = &arm.guard {
                            let ok = matches!(self.eval(guard, env)?, Value::Bool(true));
                            if !ok {
                                env.pop();
                                continue;
                            }
                        }
                        let r = self.eval(&arm.body, env);
                        env.pop();
                        return r;
                    }
                    env.pop();
                }
                panic("no match arm matched")
            }

            Expr::While { cond, body } => {
                loop {
                    match self.eval(cond, env)? {
                        Value::Bool(true) => {}
                        Value::Bool(false) => break,
                        other => {
                            return panic(format!(
                                "while condition must be bool, got {}",
                                other.type_name()
                            ))
                        }
                    }
                    match self.run_loop_body(body, env)? {
                        LoopStep::Break => break,
                        LoopStep::Continue => {}
                    }
                }
                Ok(Value::Unit)
            }

            Expr::WhileLet { pattern, expr, body } => {
                loop {
                    let v = self.eval(expr, env)?;
                    env.push();
                    let matched = self.match_pattern(pattern, &v, env)?;
                    if !matched {
                        env.pop();
                        break;
                    }
                    let step = self.run_loop_body(body, env);
                    env.pop();
                    match step? {
                        LoopStep::Break => break,
                        LoopStep::Continue => {}
                    }
                }
                Ok(Value::Unit)
            }

            Expr::For { var, start, end, inclusive, body } => {
                let s = self.eval_int(start, env)?;
                let e = self.eval_int(end, env)?;
                let mut i = s;
                loop {
                    let cont = if *inclusive { i <= e } else { i < e };
                    if !cont {
                        break;
                    }
                    env.push();
                    env.define(var.clone(), Value::Int(i));
                    let step = self.run_loop_body(body, env);
                    env.pop();
                    match step? {
                        LoopStep::Break => break,
                        LoopStep::Continue => {}
                    }
                    i += 1;
                }
                Ok(Value::Unit)
            }

            Expr::Return(opt) => {
                let v = match opt {
                    Some(e) => self.eval(e, env)?,
                    None => Value::Unit,
                };
                Err(Flow::Return(v))
            }
            Expr::Break => Err(Flow::Break),
            Expr::Continue => Err(Flow::Continue),

            Expr::Question(inner) => match self.eval(inner, env)? {
                Value::Ok(x) => Ok(*x),
                Value::Some(x) => Ok(*x),
                Value::Err(e) => Err(Flow::Return(Value::Err(e))),
                Value::None => Err(Flow::Return(Value::None)),
                other => panic(format!("`?` applied to non-Result/Option ({})", other.type_name())),
            },

            Expr::Call { callee, args } => {
                // `chan<T>()` lowers to a call whose callee is `chan::<T>`.
                if let Expr::StructLit { name, .. } = callee.as_ref() {
                    if name.starts_with("chan::<") {
                        return Ok(Value::Chan(Rc::new(RefCell::new(VecDeque::new()))));
                    }
                }
                self.eval_call(callee, args, env)
            }

            Expr::MethodCall { receiver, method, args } => {
                let recv = self.eval(receiver, env)?;
                // Channel methods (cooperative, single-threaded): send pushes to
                // the shared queue, recv pops from it, clone shares the handle.
                if let Value::Chan(q) = &recv {
                    return match method.as_str() {
                        "send" => {
                            let v = self.eval(&args[0], env)?;
                            q.borrow_mut().push_back(v);
                            Ok(Value::Unit)
                        }
                        "recv" => q.borrow_mut().pop_front().ok_or_else(|| {
                            Flow::Panic(
                                "recv on an empty channel — the interpreter runs `spawn` bodies \
                                 eagerly, so a value must be sent before it is received"
                                    .into(),
                            )
                        }),
                        // Non-blocking pop. Returns `Some(v)` when a value is
                        // available, `None` otherwise. Lets ASI loops poll a
                        // channel without panicking on the empty case — useful
                        // for fan-out workers where the consumer races the
                        // producers and needs to know when results have stopped
                        // coming, not just block on the first miss.
                        "try_recv" => Ok(match q.borrow_mut().pop_front() {
                            Some(v) => Value::Some(Box::new(v)),
                            None => Value::None,
                        }),
                        // How many values are queued and unread. Useful with
                        // try_recv for "drain everything available" loops, or
                        // as a "did the workers do any work?" probe.
                        "len" => Ok(Value::Int(q.borrow().len() as i64)),
                        "clone" => Ok(Value::Chan(q.clone())),
                        other => panic(format!("no method `{other}` on a channel")),
                    };
                }
                let mut argv = Vec::with_capacity(args.len() + 1);
                argv.push(recv);
                for a in args {
                    argv.push(self.eval(a, env)?);
                }
                let tn = argv[0].type_name();
                if let Some(f) = self.methods.get(&(tn.clone(), method.clone())) {
                    self.call_fn(f, argv)
                } else {
                    panic(format!("no method `{method}` on type `{tn}`"))
                }
            }

            Expr::FieldAccess { receiver, field } => {
                let v = self.eval(receiver, env)?;
                match v {
                    Value::Struct { fields, .. } | Value::Enum { fields, .. } => fields
                        .get(field)
                        .cloned()
                        .ok_or_else(|| Flow::Panic(format!("no field `{field}`"))),
                    Value::Tuple(items) => {
                        // `t.0`, `t.1`, … : the parser stores the digit as the
                        // field name, and the interpreter reads it as the index.
                        let i: usize = field.parse().map_err(|_| {
                            Flow::Panic(format!("tuple access expects a numeric index, got `.{field}`"))
                        })?;
                        items.get(i).cloned().ok_or_else(|| {
                            Flow::Panic(format!("tuple index {i} out of bounds (len {})", items.len()))
                        })
                    }
                    other => panic(format!("field access on non-struct ({})", other.type_name())),
                }
            }

            Expr::Tuple(elems) => {
                let mut vs = Vec::with_capacity(elems.len());
                for e in elems {
                    vs.push(self.eval(e, env)?);
                }
                Ok(Value::Tuple(vs))
            }

            Expr::Index { receiver, index } => {
                let arr = self.eval(receiver, env)?;
                let idx = self.eval_int(index, env)?;
                match arr {
                    Value::Array(items) => items
                        .get(idx as usize)
                        .cloned()
                        .ok_or_else(|| Flow::Panic(format!("index {idx} out of bounds (len {})", items.len()))),
                    other => panic(format!("indexing non-array ({})", other.type_name())),
                }
            }

            Expr::Array(elems) => {
                let mut out = Vec::with_capacity(elems.len());
                for e in elems {
                    out.push(self.eval(e, env)?);
                }
                Ok(Value::Array(out))
            }

            Expr::StructLit { name, fields } => {
                let mut fmap = HashMap::with_capacity(fields.len());
                for (fname, fexpr) in fields {
                    fmap.insert(fname.clone(), self.eval(fexpr, env)?);
                }
                if let Some((enum_name, variant)) = name.split_once("::") {
                    Ok(Value::Enum {
                        enum_name: enum_name.to_string(),
                        variant: variant.to_string(),
                        fields: fmap,
                    })
                } else {
                    Ok(Value::Struct { name: name.clone(), fields: fmap })
                }
            }

            Expr::Ok(e) => Ok(Value::Ok(Box::new(self.eval(e, env)?))),
            Expr::Err(e) => Ok(Value::Err(Box::new(self.eval(e, env)?))),
            Expr::Some(e) => Ok(Value::Some(Box::new(self.eval(e, env)?))),
            Expr::None => Ok(Value::None),

            Expr::FmtStr { parts } => {
                use crate::ast::FmtPart;
                let mut s = String::new();
                for part in parts {
                    match part {
                        FmtPart::Lit(t) => s.push_str(t),
                        FmtPart::Expr(e) => {
                            let v = self.eval(e, env)?;
                            s.push_str(&display(&v));
                        }
                    }
                }
                Ok(Value::Str(s))
            }

            Expr::Lambda { params, body, .. } => Ok(Value::Closure {
                params: params.iter().map(|p| p.name.clone()).collect(),
                body: body.clone(),
                captured: env.snapshot(),
            }),

            Expr::Comptime(inner) => self.eval(inner, env),

            // Cooperative concurrency: run the spawned body eagerly (single-
            // threaded), so its sends are queued before the main flow continues.
            Expr::Spawn(body) => {
                self.eval(body, env)?;
                Ok(Value::Unit)
            }
            // Cooperative select: fire the first arm whose channel has a ready
            // value (its queue is non-empty), consuming that value and running the
            // arm body. Arms are `c.recv() => body`. With eager `spawn`, channels
            // are pre-filled, so this is deterministic (first ready arm wins).
            Expr::Select(arms) => {
                for arm in arms {
                    let Expr::MethodCall { receiver, method, .. } = &arm.recv else {
                        return panic("select arms must be channel `recv()` operations");
                    };
                    if method != "recv" {
                        return panic("select arms must be channel `recv()` operations");
                    }
                    let Value::Chan(q) = self.eval(receiver, env)? else {
                        return panic("select arm `recv` on a non-channel");
                    };
                    let ready = q.borrow_mut().pop_front();
                    if ready.is_some() {
                        return self.eval(&arm.body, env);
                    }
                }
                panic("select: no channel was ready (cooperative interpreter — send before select)")
            }
        }
    }

    fn eval_block(&self, stmts: &[Stmt], env: &mut Env) -> R {
        env.push();
        let mut last = Value::Unit;
        for stmt in stmts {
            match self.eval(&stmt.expr, env) {
                Ok(v) => last = v,
                Err(e) => {
                    env.pop();
                    return Err(e);
                }
            }
        }
        env.pop();
        Ok(last)
    }

    /// Run a loop body (a `Vec<Stmt>`) in a fresh scope, translating `break`
    /// and `continue` into a [`LoopStep`] for the caller's loop construct.
    fn run_loop_body(&self, body: &[Stmt], env: &mut Env) -> Result<LoopStep, Flow> {
        env.push();
        for stmt in body {
            match self.eval(&stmt.expr, env) {
                Ok(_) => {}
                Err(Flow::Break) => {
                    env.pop();
                    return Ok(LoopStep::Break);
                }
                Err(Flow::Continue) => {
                    env.pop();
                    return Ok(LoopStep::Continue);
                }
                Err(e) => {
                    env.pop();
                    return Err(e);
                }
            }
        }
        env.pop();
        Ok(LoopStep::Continue)
    }

    fn eval_call(&self, callee: &Expr, args: &[Expr], env: &mut Env) -> R {
        // Evaluate arguments left-to-right.
        let mut argv = Vec::with_capacity(args.len());
        for a in args {
            argv.push(self.eval(a, env)?);
        }

        if let Expr::Ident(name) = callee {
            // 1. A local/captured variable holding a closure.
            if let Some(Value::Closure { .. }) = env.get(name) {
                let c = env.get(name).unwrap().clone();
                return self.call_closure(c, argv);
            }
            // 2. A builtin.
            if let Some(v) = self.call_builtin(name, &argv)? {
                return Ok(v);
            }
            // 3. A user-defined function.
            if let Some(f) = self.fns.get(name) {
                return self.call_fn(f, argv);
            }
            // 4. A module-level closure constant.
            if let Some(Value::Closure { .. }) = self.globals.get(name) {
                let c = self.globals.get(name).unwrap().clone();
                return self.call_closure(c, argv);
            }
            return panic(format!("call to unknown function `{name}`"));
        }

        // Callee is an expression that should evaluate to a closure
        // (e.g. `make_adder(1)(2)` or an array element).
        let c = self.eval(callee, env)?;
        self.call_closure(c, argv)
    }

    fn eval_binop(&self, op: &BinOp, left: &Expr, right: &Expr, env: &mut Env) -> R {
        // Short-circuit boolean operators.
        match op {
            BinOp::And => {
                return match self.eval(left, env)? {
                    Value::Bool(false) => Ok(Value::Bool(false)),
                    Value::Bool(true) => match self.eval(right, env)? {
                        Value::Bool(b) => Ok(Value::Bool(b)),
                        other => panic(format!("`&&` rhs must be bool, got {}", other.type_name())),
                    },
                    other => panic(format!("`&&` lhs must be bool, got {}", other.type_name())),
                };
            }
            BinOp::Or => {
                return match self.eval(left, env)? {
                    Value::Bool(true) => Ok(Value::Bool(true)),
                    Value::Bool(false) => match self.eval(right, env)? {
                        Value::Bool(b) => Ok(Value::Bool(b)),
                        other => panic(format!("`||` rhs must be bool, got {}", other.type_name())),
                    },
                    other => panic(format!("`||` lhs must be bool, got {}", other.type_name())),
                };
            }
            _ => {}
        }

        let l = self.eval(left, env)?;
        let r = self.eval(right, env)?;
        eval_binop_vals(op, l, r)
    }

    fn eval_int(&self, expr: &Expr, env: &mut Env) -> Result<i64, Flow> {
        match self.eval(expr, env)? {
            Value::Int(n) => Ok(n),
            other => panic(format!("expected i64, got {}", other.type_name())),
        }
    }

    // ── Pattern matching ─────────────────────────────────────────────────────

    /// Try to match `val` against `pat`, binding identifiers into the current
    /// scope of `env`. Returns whether it matched.
    fn match_pattern(&self, pat: &Pattern, val: &Value, env: &mut Env) -> Result<bool, Flow> {
        match pat {
            Pattern::Wildcard => Ok(true),
            Pattern::Ident(name) => {
                env.define(name.clone(), val.clone());
                Ok(true)
            }
            Pattern::Literal(lit) => Ok(values_equal(&lit_to_val(lit), val)),
            Pattern::Some(inner) => match val {
                Value::Some(v) => self.match_pattern(inner, v, env),
                _ => Ok(false),
            },
            Pattern::None => Ok(matches!(val, Value::None)),
            Pattern::Ok(inner) => match val {
                Value::Ok(v) => self.match_pattern(inner, v, env),
                _ => Ok(false),
            },
            Pattern::Err(inner) => match val {
                Value::Err(v) => self.match_pattern(inner, v, env),
                _ => Ok(false),
            },
            Pattern::Struct { name, fields } => {
                // Enum-variant pattern when the name is qualified (`Enum::Variant`).
                let field_map = if let Some((enum_name, variant)) = name.split_once("::") {
                    match val {
                        Value::Enum { enum_name: en, variant: v, fields }
                            if en == enum_name && v == variant =>
                        {
                            fields
                        }
                        _ => return Ok(false),
                    }
                } else {
                    match val {
                        Value::Struct { name: sn, fields } if sn == name => fields,
                        _ => return Ok(false),
                    }
                };
                for (fname, fpat) in fields {
                    let Some(fval) = field_map.get(fname) else {
                        return Ok(false);
                    };
                    let fval = fval.clone();
                    if !self.match_pattern(fpat, &fval, env)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
            Pattern::Tuple(pats) => {
                let Value::Tuple(items) = val else { return Ok(false) };
                if items.len() != pats.len() {
                    return Ok(false);
                }
                for (p, v) in pats.iter().zip(items.iter()) {
                    let v = v.clone();
                    if !self.match_pattern(p, &v, env)? {
                        return Ok(false);
                    }
                }
                Ok(true)
            }
        }
    }

    // ── Builtins ───────────────────────────────────────────────────────────────

    /// Dispatch a builtin call. Returns `Ok(Some(v))` if `name` is a builtin,
    /// `Ok(None)` if it is not (caller should try user functions).
    fn call_builtin(&self, name: &str, args: &[Value]) -> Result<Option<Value>, Flow> {
        // Helpers --------------------------------------------------------------
        let want = |n: usize| -> Result<(), Flow> {
            if args.len() == n {
                Ok(())
            } else {
                Err(Flow::Panic(format!("{name}: expected {n} args, got {}", args.len())))
            }
        };
        macro_rules! ok {
            ($v:expr) => {
                return Ok(Some($v))
            };
        }

        match name {
            // ── I/O ───────────────────────────────────────────────────────────
            "print" => {
                want(1)?;
                print!("{}", display(&args[0]));
                let _ = std::io::stdout().flush();
                ok!(Value::Unit);
            }
            "println" => {
                want(1)?;
                println!("{}", display(&args[0]));
                ok!(Value::Unit);
            }
            "eprint" => {
                want(1)?;
                eprint!("{}", display(&args[0]));
                ok!(Value::Unit);
            }
            "eprintln" => {
                want(1)?;
                eprintln!("{}", display(&args[0]));
                ok!(Value::Unit);
            }
            "read_line" => {
                want(0)?;
                let mut line = String::new();
                match std::io::stdin().read_line(&mut line) {
                    Ok(_) => {
                        while line.ends_with('\n') || line.ends_with('\r') {
                            line.pop();
                        }
                        ok!(Value::Str(line));
                    }
                    Err(e) => ok!(Value::Str(format!("<read error: {e}>"))),
                }
            }
            "read_file" => {
                want(1)?;
                match std::fs::read_to_string(as_str(&args[0])?) {
                    Ok(s) => ok!(Value::Ok(Box::new(Value::Str(s)))),
                    Err(e) => ok!(Value::Err(Box::new(Value::Str(e.to_string())))),
                }
            }
            "write_file" => {
                want(2)?;
                match std::fs::write(as_str(&args[0])?, as_str(&args[1])?) {
                    Ok(()) => ok!(Value::Ok(Box::new(Value::Unit))),
                    Err(e) => ok!(Value::Err(Box::new(Value::Str(e.to_string())))),
                }
            }

            // ── Conversion / formatting ─────────────────────────────────────────
            "to_str" => {
                want(1)?;
                ok!(Value::Str(as_int(&args[0])?.to_string()));
            }
            "to_str_f64" => {
                want(1)?;
                ok!(Value::Str(fmt_g(as_float(&args[0])?)));
            }
            "to_str_bool" => {
                want(1)?;
                ok!(Value::Str(if as_bool(&args[0])? { "true" } else { "false" }.to_string()));
            }
            "i64_to_str" => {
                want(1)?;
                ok!(Value::Str(as_int(&args[0])?.to_string()));
            }
            "format" => {
                want(1)?;
                // Interpolation is lowered at parse time; `format` is the identity
                // on an already-interpolated string.
                ok!(Value::Str(as_str(&args[0])?.to_string()));
            }
            "parse_int" => {
                want(1)?;
                ok!(match as_str(&args[0])?.trim().parse::<i64>() {
                    Ok(n) => Value::Ok(Box::new(Value::Int(n))),
                    Err(_) => Value::Err(Box::new(Value::Str("parse error".into()))),
                });
            }
            "parse_float" => {
                want(1)?;
                ok!(match as_str(&args[0])?.trim().parse::<f64>() {
                    Ok(f) => Value::Ok(Box::new(Value::Float(f))),
                    Err(_) => Value::Err(Box::new(Value::Str("parse error".into()))),
                });
            }
            "parse_bool" => {
                want(1)?;
                ok!(match as_str(&args[0])?.trim() {
                    "true" => Value::Ok(Box::new(Value::Bool(true))),
                    "false" => Value::Ok(Box::new(Value::Bool(false))),
                    _ => Value::Err(Box::new(Value::Str("parse error".into()))),
                });
            }
            // Parse-with-default variants that fold the Result-match
            // ceremony away. Useful in load-from-disk paths where
            // a missing or malformed value should fall back silently
            // rather than propagate an Err the caller has to unwrap.
            "parse_int_or" => {
                want(2)?;
                let n = as_str(&args[0])?
                    .trim()
                    .parse::<i64>()
                    .unwrap_or(as_int(&args[1])?);
                ok!(Value::Int(n));
            }
            "parse_float_or" => {
                want(2)?;
                let f = as_str(&args[0])?
                    .trim()
                    .parse::<f64>()
                    .unwrap_or(as_float(&args[1])?);
                ok!(Value::Float(f));
            }
            "parse_bool_or" => {
                want(2)?;
                let b = match as_str(&args[0])?.trim() {
                    "true" => true,
                    "false" => false,
                    _ => match &args[1] {
                        Value::Bool(b) => *b,
                        other => return panic(format!(
                            "parse_bool_or: default must be bool, got {}",
                            other.type_name()
                        )),
                    },
                };
                ok!(Value::Bool(b));
            }
            // Keep only ASCII digits; everything else is dropped. Closes
            // ROADMAP §9.5 F7 — gives string-shape demos (phone numbers,
            // codes, IDs) a one-liner alternative to pushing parsing onto
            // an LLM. Composes with parse_int: parse_int(str_digits_only(s)).
            "str_digits_only" => {
                want(1)?;
                let s = as_str(&args[0])?;
                let out: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
                ok!(Value::Str(out));
            }
            "len" => {
                want(1)?;
                ok!(match &args[0] {
                    Value::Str(s) => Value::Int(s.len() as i64),
                    Value::Array(a) => Value::Int(a.len() as i64),
                    other => return panic(format!("len: expected str/array, got {}", other.type_name())),
                });
            }

            // ── Math ────────────────────────────────────────────────────────────
            "abs_i32" | "abs_i64" => {
                want(1)?;
                ok!(Value::Int(as_int(&args[0])?.abs()));
            }

            // ── Array helpers ─────────────────────────────────────────────────
            // Concrete-typed because the interpreter's array path holds
            // `Vec<Value>` and the inference layer wants concrete return shapes;
            // generic [T] forms wait on Phase-8 search-strategy work that also
            // teaches the optimizer about user-defined domains.

            // Half-open range `[start, end)`. Returns an empty slice when
            // `end <= start`. Saturates element count silently if asked for an
            // implausibly large range — caller should size with awareness.
            "arr_range" => {
                want(2)?;
                let start = as_int(&args[0])?;
                let end = as_int(&args[1])?;
                if end <= start {
                    ok!(Value::Array(Vec::new()));
                }
                let len = (end - start) as usize;
                let mut out = Vec::with_capacity(len.min(1 << 20));
                let mut i = start;
                while i < end && out.len() < (1 << 20) {
                    out.push(Value::Int(i));
                    i += 1;
                }
                ok!(Value::Array(out));
            }
            // Append: returns a fresh array with `x` at the end. Copy
            // semantics — the input array is unaffected.
            "arr_push" => {
                want(2)?;
                let mut xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_push: expected array, got {}",
                        other.type_name()
                    )),
                };
                xs.push(args[1].clone());
                ok!(Value::Array(xs));
            }
            // Sum of an i64 array. Empty → 0. Saturates on overflow.
            "arr_sum_i64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_sum_i64: expected array, got {}",
                        other.type_name()
                    )),
                };
                let mut s: i64 = 0;
                for v in xs {
                    let n = match v {
                        Value::Int(n) => *n,
                        other => return panic(format!(
                            "arr_sum_i64: element must be i64, got {}",
                            other.type_name()
                        )),
                    };
                    s = s.saturating_add(n);
                }
                ok!(Value::Int(s));
            }
            // Map a closure / lambda across an array, producing a fresh
            // array of the results. The closure runs through `call_closure`
            // so it sees its captured environment, and any `Flow::Panic`
            // it raises is bubbled up — failures aren't swallowed.
            "arr_map" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_map: expected array, got {}",
                        other.type_name()
                    )),
                };
                let f = args[1].clone();
                let mut out = Vec::with_capacity(xs.len());
                for x in xs {
                    let mapped = self.call_closure(f.clone(), vec![x])?;
                    out.push(mapped);
                }
                ok!(Value::Array(out));
            }
            // Reduce an array to a single value via `f(acc, x) -> acc`.
            // The most general functional combinator — arr_sum_i64, arr_max,
            // arr_min, count, product, etc. are all special cases.
            "arr_fold" => {
                want(3)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_fold: expected array, got {}",
                        other.type_name()
                    )),
                };
                let mut acc = args[1].clone();
                let f = args[2].clone();
                for x in xs {
                    acc = self.call_closure(f.clone(), vec![acc, x])?;
                }
                ok!(acc);
            }
            // Sort an array via a comparator closure `(a, b) -> i64` with
            // standard cmp semantics (neg = a<b, 0 = eq, pos = a>b).
            // Stable sort (insertion-sort under the hood for simplicity);
            // not big-O optimal but plenty for ASI-scale arrays. Returns a
            // fresh sorted array — input untouched.
            "arr_sort_by" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_sort_by: expected array, got {}",
                        other.type_name()
                    )),
                };
                let cmp = args[1].clone();
                // Insertion sort. Each comparison hits call_closure;
                // O(n²) on length but the closure dispatch dominates so
                // a fancier algorithm wouldn't move the needle here.
                let mut out: Vec<Value> = Vec::with_capacity(xs.len());
                for x in xs {
                    let mut lo = 0;
                    let hi = out.len();
                    // Linear probe (binary search would re-run the cmp for
                    // already-sorted items; for typical ASI use n is small).
                    while lo < hi {
                        let r = self.call_closure(cmp.clone(), vec![x.clone(), out[lo].clone()])?;
                        let r = match r {
                            Value::Int(n) => n,
                            other => return panic(format!(
                                "arr_sort_by: comparator must return i64, got {}",
                                other.type_name()
                            )),
                        };
                        if r < 0 { break; }
                        lo += 1;
                    }
                    out.insert(lo, x);
                }
                ok!(Value::Array(out));
            }
            // Build an array by repeating `v` `n` times. Common need:
            // initialize a fresh array with a default value before mutating
            // it in place. Polymorphic via `T`.
            "arr_repeat" => {
                want(2)?;
                let v = args[0].clone();
                let n = as_int(&args[1])?.max(0) as usize;
                ok!(Value::Array(vec![v; n.min(1 << 20)]));
            }
            // Concatenate two arrays into a fresh one. Element types must
            // agree at the runtime — we don't do conversions here. The
            // result preserves order: `xs ++ ys`.
            "arr_concat" => {
                want(2)?;
                let mut out = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_concat: expected array, got {}",
                        other.type_name()
                    )),
                };
                let ys = match &args[1] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_concat: expected array, got {}",
                        other.type_name()
                    )),
                };
                out.reserve(ys.len());
                for v in ys {
                    out.push(v.clone());
                }
                ok!(Value::Array(out));
            }
            // Flatten `[[T]] -> [T]`. Each inner element must itself be an
            // array; mixed shapes panic. Useful after `arr_map` produces
            // nested results.
            "arr_flatten" => {
                want(1)?;
                let xss = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_flatten: expected array of arrays, got {}",
                        other.type_name()
                    )),
                };
                let total: usize = xss
                    .iter()
                    .map(|v| match v {
                        Value::Array(inner) => inner.len(),
                        _ => 0,
                    })
                    .sum();
                let mut out = Vec::with_capacity(total);
                for v in xss {
                    match v {
                        Value::Array(inner) => {
                            for x in inner {
                                out.push(x.clone());
                            }
                        }
                        other => return panic(format!(
                            "arr_flatten: inner element must be array, got {}",
                            other.type_name()
                        )),
                    }
                }
                ok!(Value::Array(out));
            }

            // Numeric `as`-style casts. Concrete builtins that work on
            // either i64 or f64 input — let the value's runtime type drive.
            // Pairs with the existing `i64_to_f64` / `f64_to_i64` but is
            // polymorphic on the source so ASI demos don't need to know
            // the source type at the call site.
            "as_f64" => {
                want(1)?;
                let f = match &args[0] {
                    Value::Int(n) => *n as f64,
                    Value::Float(v) => *v,
                    Value::Bool(b) => if *b { 1.0 } else { 0.0 },
                    other => return panic(format!(
                        "as_f64: expected i64/f64/bool, got {}",
                        other.type_name()
                    )),
                };
                ok!(Value::Float(f));
            }
            "as_i64" => {
                want(1)?;
                let n = match &args[0] {
                    Value::Int(n) => *n,
                    Value::Float(v) => *v as i64,
                    Value::Bool(b) => if *b { 1 } else { 0 },
                    other => return panic(format!(
                        "as_i64: expected i64/f64/bool, got {}",
                        other.type_name()
                    )),
                };
                ok!(Value::Int(n));
            }

            // ── Polymorphic slicing / reordering ──────────────────────────
            "arr_reverse" => {
                want(1)?;
                let mut xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_reverse: expected array, got {}",
                        other.type_name()
                    )),
                };
                xs.reverse();
                ok!(Value::Array(xs));
            }
            "arr_take" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_take: expected array, got {}",
                        other.type_name()
                    )),
                };
                let n = as_int(&args[1])?.max(0) as usize;
                let take = n.min(xs.len());
                ok!(Value::Array(xs[..take].to_vec()));
            }
            "arr_drop" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_drop: expected array, got {}",
                        other.type_name()
                    )),
                };
                let n = as_int(&args[1])?.max(0) as usize;
                let skip = n.min(xs.len());
                ok!(Value::Array(xs[skip..].to_vec()));
            }

            // ── f64 array reductions (mirrors arr_*_i64) ──────────────────
            "arr_sum_f64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_sum_f64: expected array, got {}",
                        other.type_name()
                    )),
                };
                let mut s = 0.0_f64;
                for v in xs {
                    let f = match v {
                        Value::Float(f) => *f,
                        Value::Int(n) => *n as f64,
                        other => return panic(format!(
                            "arr_sum_f64: element must be numeric, got {}",
                            other.type_name()
                        )),
                    };
                    s += f;
                }
                ok!(Value::Float(s));
            }
            // Mean of an i64 array → f64 (almost never an integer). Empty
            // → 0.0 rather than NaN/panic; caller should guard if zero is
            // meaningful for them.
            "arr_mean_i64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_mean_i64: expected array, got {}",
                        other.type_name()
                    )),
                };
                if xs.is_empty() { ok!(Value::Float(0.0)); }
                let mut s: i64 = 0;
                for v in xs {
                    let n = match v {
                        Value::Int(n) => *n,
                        other => return panic(format!(
                            "arr_mean_i64: element must be i64, got {}",
                            other.type_name()
                        )),
                    };
                    s = s.saturating_add(n);
                }
                ok!(Value::Float(s as f64 / xs.len() as f64));
            }
            // Mean of an f64 (or i64-coerced) array.
            "arr_mean_f64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_mean_f64: expected array, got {}",
                        other.type_name()
                    )),
                };
                if xs.is_empty() { ok!(Value::Float(0.0)); }
                let mut s = 0.0_f64;
                for v in xs {
                    let f = match v {
                        Value::Float(f) => *f,
                        Value::Int(n) => *n as f64,
                        other => return panic(format!(
                            "arr_mean_f64: element must be numeric, got {}",
                            other.type_name()
                        )),
                    };
                    s += f;
                }
                ok!(Value::Float(s / xs.len() as f64));
            }
            // Sample standard deviation: sqrt of (sum of squared deviations
            // / (n - 1)). `n < 2` panics — std on a single sample is
            // undefined; caller should guard.
            "arr_std_f64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_std_f64: expected array, got {}",
                        other.type_name()
                    )),
                };
                if xs.len() < 2 {
                    return panic("arr_std_f64: need at least 2 samples".to_string());
                }
                let mut sum = 0.0_f64;
                let mut fs: Vec<f64> = Vec::with_capacity(xs.len());
                for v in xs {
                    let f = match v {
                        Value::Float(f) => *f,
                        Value::Int(n) => *n as f64,
                        other => return panic(format!(
                            "arr_std_f64: element must be numeric, got {}",
                            other.type_name()
                        )),
                    };
                    sum += f;
                    fs.push(f);
                }
                let mean = sum / fs.len() as f64;
                let mut var_acc = 0.0_f64;
                for f in &fs {
                    let d = *f - mean;
                    var_acc += d * d;
                }
                let variance = var_acc / (fs.len() - 1) as f64;
                ok!(Value::Float(variance.sqrt()));
            }
            // Index of the largest / smallest element. Empty array panics
            // (no sensible default). Ties broken by lowest index.
            "arr_argmax_i64" | "arr_argmin_i64" | "arr_argmax_f64" | "arr_argmin_f64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "{name}: expected array, got {}",
                        other.type_name()
                    )),
                };
                if xs.is_empty() {
                    return panic(format!("{name}: array is empty"));
                }
                let pick_max = name == "arr_argmax_i64" || name == "arr_argmax_f64";
                let as_f = |v: &Value| -> Result<f64, Flow> {
                    match v {
                        Value::Int(n) => Ok(*n as f64),
                        Value::Float(f) => Ok(*f),
                        other => Err(Flow::Panic(format!(
                            "{name}: element must be numeric, got {}",
                            other.type_name()
                        ))),
                    }
                };
                let mut best_idx = 0;
                let mut best_val = as_f(&xs[0])?;
                for (i, v) in xs.iter().enumerate().skip(1) {
                    let f = as_f(v)?;
                    if (pick_max && f > best_val) || (!pick_max && f < best_val) {
                        best_val = f;
                        best_idx = i;
                    }
                }
                ok!(Value::Int(best_idx as i64));
            }
            "arr_max_f64" | "arr_min_f64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "{name}: expected array, got {}",
                        other.type_name()
                    )),
                };
                if xs.is_empty() {
                    return panic(format!("{name}: array is empty"));
                }
                let pick_max = name == "arr_max_f64";
                let as_f = |v: &Value| -> Result<f64, Flow> {
                    match v {
                        Value::Float(f) => Ok(*f),
                        Value::Int(n) => Ok(*n as f64),
                        other => Err(Flow::Panic(format!(
                            "{name}: element must be numeric, got {}",
                            other.type_name()
                        ))),
                    }
                };
                let mut best = as_f(&xs[0])?;
                for v in &xs[1..] {
                    let f = as_f(v)?;
                    if (pick_max && f > best) || (!pick_max && f < best) {
                        best = f;
                    }
                }
                ok!(Value::Float(best));
            }

            // ── String split / join ───────────────────────────────────────
            // str_split("a,b,c", ",") → ["a", "b", "c"]. Empty separator
            // returns the input as a single-element slice (matches Rust's
            // is-not-allowed semantics by sidestepping the panic).
            "str_split" => {
                want(2)?;
                let s = as_str(&args[0])?;
                let sep = as_str(&args[1])?;
                let parts: Vec<Value> = if sep.is_empty() {
                    vec![Value::Str(s.to_string())]
                } else {
                    s.split(sep).map(|p| Value::Str(p.to_string())).collect()
                };
                ok!(Value::Array(parts));
            }
            // str_join(["a","b","c"], "-") → "a-b-c". Non-string elements
            // panic — caller should arr_map(to_str(x)) first if needed.
            "str_join" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "str_join: expected array, got {}",
                        other.type_name()
                    )),
                };
                let sep = as_str(&args[1])?.to_string();
                let mut parts = Vec::with_capacity(xs.len());
                for v in xs {
                    match v {
                        Value::Str(s) => parts.push(s.clone()),
                        other => return panic(format!(
                            "str_join: element must be str, got {}",
                            other.type_name()
                        )),
                    }
                }
                ok!(Value::Str(parts.join(&sep)));
            }

            // Pair two arrays element-wise into a `[(a, b)]` slice; truncates
            // to the shorter input. Composes with arr_map / arr_filter for
            // dataset zipping (features + labels, etc.).
            "arr_zip" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_zip: expected array, got {}",
                        other.type_name()
                    )),
                };
                let ys = match &args[1] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_zip: expected array, got {}",
                        other.type_name()
                    )),
                };
                let n = xs.len().min(ys.len());
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    out.push(Value::Tuple(vec![xs[i].clone(), ys[i].clone()]));
                }
                ok!(Value::Array(out));
            }
            // Split an array into consecutive chunks of size `n`. The last
            // chunk may be shorter if `len(xs)` isn't a multiple of `n`.
            // `n <= 0` panics — caller should guard with their own min.
            "arr_chunk" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_chunk: expected array, got {}",
                        other.type_name()
                    )),
                };
                let n = as_int(&args[1])?;
                if n <= 0 {
                    return panic(format!("arr_chunk: chunk size must be positive, got {n}"));
                }
                let n = n as usize;
                let mut out: Vec<Value> = Vec::with_capacity(xs.len().div_ceil(n));
                let mut start = 0;
                while start < xs.len() {
                    let end = (start + n).min(xs.len());
                    out.push(Value::Array(xs[start..end].to_vec()));
                    start = end;
                }
                ok!(Value::Array(out));
            }
            // Dedupe an array, preserving first occurrence and order. Uses
            // structural equality so deeply-nested values dedupe correctly.
            // O(n²) — fine for ASI-scale arrays; a hash-based version waits
            // on a HashMap/HashSet primitive.
            "arr_unique" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_unique: expected array, got {}",
                        other.type_name()
                    )),
                };
                let mut out: Vec<Value> = Vec::new();
                for v in xs {
                    if !out.iter().any(|seen| values_equal(seen, v)) {
                        out.push(v.clone());
                    }
                }
                ok!(Value::Array(out));
            }
            // First index where the element structurally equals `v`. Returns
            // `Some(i)` if found, `None` otherwise. Pairs with arr_contains
            // for "where's the match" follow-ups.
            "arr_index_of" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_index_of: expected array, got {}",
                        other.type_name()
                    )),
                };
                let needle = &args[1];
                let mut found: Option<i64> = None;
                for (i, v) in xs.iter().enumerate() {
                    if values_equal(v, needle) {
                        found = Some(i as i64);
                        break;
                    }
                }
                ok!(match found {
                    Some(i) => Value::Some(Box::new(Value::Int(i))),
                    None => Value::None,
                });
            }
            // `arr_any(xs, pred)` — does at least one element satisfy the
            // predicate? Short-circuits on the first true. The bool dual
            // of arr_find: arr_find returns the element, arr_any returns
            // whether one exists.
            "arr_any" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_any: expected array, got {}",
                        other.type_name()
                    )),
                };
                let pred = args[1].clone();
                let mut hit = false;
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x])?;
                    match r {
                        Value::Bool(true) => { hit = true; break; }
                        Value::Bool(false) => {}
                        other => return panic(format!(
                            "arr_any: predicate must return bool, got {}",
                            other.type_name()
                        )),
                    }
                }
                ok!(Value::Bool(hit));
            }
            // `arr_all(xs, pred)` — do ALL elements satisfy the predicate?
            // Short-circuits on the first false. Empty array → true
            // (vacuous truth, matches mathematical convention).
            "arr_all" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_all: expected array, got {}",
                        other.type_name()
                    )),
                };
                let pred = args[1].clone();
                let mut all = true;
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x])?;
                    match r {
                        Value::Bool(true) => {}
                        Value::Bool(false) => { all = false; break; }
                        other => return panic(format!(
                            "arr_all: predicate must return bool, got {}",
                            other.type_name()
                        )),
                    }
                }
                ok!(Value::Bool(all));
            }
            // `arr_count_if(xs, pred)` — count elements where the
            // predicate returns true. Equivalent to `len(arr_filter(xs,
            // pred))` but doesn't materialize the filtered array.
            "arr_count_if" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_count_if: expected array, got {}",
                        other.type_name()
                    )),
                };
                let pred = args[1].clone();
                let mut n: i64 = 0;
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x])?;
                    match r {
                        Value::Bool(true) => { n += 1; }
                        Value::Bool(false) => {}
                        other => return panic(format!(
                            "arr_count_if: predicate must return bool, got {}",
                            other.type_name()
                        )),
                    }
                }
                ok!(Value::Int(n));
            }
            // `arr_zip_with(xs, ys, f)` — pair element-wise then map via
            // a 2-arg closure: `f(x, y) -> z`. Truncates to the shorter
            // input. More efficient than `arr_zip` + `arr_map` (no
            // intermediate tuple slice) and lets the closure see both
            // values without destructuring.
            "arr_zip_with" => {
                want(3)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_zip_with: expected array, got {}",
                        other.type_name()
                    )),
                };
                let ys = match &args[1] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_zip_with: expected array, got {}",
                        other.type_name()
                    )),
                };
                let f = args[2].clone();
                let n = xs.len().min(ys.len());
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let z = self.call_closure(
                        f.clone(),
                        vec![xs[i].clone(), ys[i].clone()],
                    )?;
                    out.push(z);
                }
                ok!(Value::Array(out));
            }
            // First element matching the predicate closure. Returns
            // `Some(v)` when one is found, `None` otherwise — the
            // closure shape mirrors arr_filter, but the result is a
            // single element so callers can early-exit.
            "arr_find" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_find: expected array, got {}",
                        other.type_name()
                    )),
                };
                let pred = args[1].clone();
                let mut hit: Option<Value> = None;
                for x in xs {
                    let keep = self.call_closure(pred.clone(), vec![x.clone()])?;
                    match keep {
                        Value::Bool(true) => { hit = Some(x); break; }
                        Value::Bool(false) => {}
                        other => return panic(format!(
                            "arr_find: predicate must return bool, got {}",
                            other.type_name()
                        )),
                    }
                }
                ok!(match hit {
                    Some(v) => Value::Some(Box::new(v)),
                    None => Value::None,
                });
            }
            // Linear scan: does `xs` contain a value equal to `v`?
            // Structural equality via the existing `values_equal` helper —
            // works for primitives, strings, tuples, nested arrays.
            "arr_contains" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_contains: expected array, got {}",
                        other.type_name()
                    )),
                };
                let needle = &args[1];
                let mut found = false;
                for v in xs {
                    if values_equal(v, needle) {
                        found = true;
                        break;
                    }
                }
                ok!(Value::Bool(found));
            }
            // Filter an array by a predicate closure (`i64 -> bool`). Keeps
            // elements where the closure returns `true`. Closures that
            // don't return bool panic, surfacing a typing mistake.
            "arr_filter" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_filter: expected array, got {}",
                        other.type_name()
                    )),
                };
                let f = args[1].clone();
                let mut out = Vec::with_capacity(xs.len());
                for x in xs {
                    let keep = self.call_closure(f.clone(), vec![x.clone()])?;
                    match keep {
                        Value::Bool(true) => out.push(x),
                        Value::Bool(false) => {}
                        other => return panic(format!(
                            "arr_filter: predicate must return bool, got {}",
                            other.type_name()
                        )),
                    }
                }
                ok!(Value::Array(out));
            }
            // Max / min of an i64 array. Empty → panic (no sensible default
            // for an unbounded domain; caller should `if len(xs) > 0` first).
            "arr_max_i64" | "arr_min_i64" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "{name}: expected array, got {}",
                        other.type_name()
                    )),
                };
                if xs.is_empty() {
                    return panic(format!("{name}: array is empty"));
                }
                let pick_max = name == "arr_max_i64";
                let mut best: i64 = match &xs[0] {
                    Value::Int(n) => *n,
                    other => return panic(format!(
                        "{name}: element must be i64, got {}",
                        other.type_name()
                    )),
                };
                for v in &xs[1..] {
                    let n = match v {
                        Value::Int(n) => *n,
                        other => return panic(format!(
                            "{name}: element must be i64, got {}",
                            other.type_name()
                        )),
                    };
                    if (pick_max && n > best) || (!pick_max && n < best) {
                        best = n;
                    }
                }
                ok!(Value::Int(best));
            }
            "abs_f64" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.abs()));
            }
            "min_i32" | "min_i64" => {
                want(2)?;
                ok!(Value::Int(as_int(&args[0])?.min(as_int(&args[1])?)));
            }
            "max_i32" | "max_i64" => {
                want(2)?;
                ok!(Value::Int(as_int(&args[0])?.max(as_int(&args[1])?)));
            }
            "sqrt" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.sqrt()));
            }
            "pow" => {
                want(2)?;
                ok!(Value::Float(as_float(&args[0])?.powf(as_float(&args[1])?)));
            }
            "exp" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.exp()));
            }
            // Natural log. `x <= 0` returns NaN (matches Rust's f64::ln);
            // callers should guard if zero is plausible — UCB-style
            // algorithms compute ln(t) where t starts at 1, so this is
            // fine in practice.
            "ln" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.ln()));
            }
            // Base-10 log, for human-readable scales (orders of magnitude).
            "log10" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.log10()));
            }
            "floor" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.floor()));
            }
            "ceil" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.ceil()));
            }
            "min_f64" => {
                want(2)?;
                ok!(Value::Float(as_float(&args[0])?.min(as_float(&args[1])?)));
            }
            "max_f64" => {
                want(2)?;
                ok!(Value::Float(as_float(&args[0])?.max(as_float(&args[1])?)));
            }
            "clamp_i64" => {
                want(3)?;
                let (n, lo, hi) = (as_int(&args[0])?, as_int(&args[1])?, as_int(&args[2])?);
                ok!(Value::Int(n.max(lo).min(hi)));
            }
            "clamp_f64" => {
                want(3)?;
                let (n, lo, hi) = (as_float(&args[0])?, as_float(&args[1])?, as_float(&args[2])?);
                ok!(Value::Float(n.max(lo).min(hi)));
            }
            "sign_i64" => {
                want(1)?;
                ok!(Value::Int(as_int(&args[0])?.signum()));
            }
            "pow_i64" => {
                want(2)?;
                let (base, exp) = (as_int(&args[0])?, as_int(&args[1])?);
                if exp < 0 {
                    return panic("pow_i64: negative exponent");
                }
                ok!(Value::Int(base.wrapping_pow(exp as u32)));
            }
            "sqrt_f64" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.sqrt()));
            }
            "floor_f64" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.floor()));
            }
            "ceil_f64" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.ceil()));
            }
            "round_f64" => {
                want(1)?;
                ok!(Value::Float(as_float(&args[0])?.round()));
            }
            "random_f64" => {
                want(0)?;
                // 53-bit mantissa → uniform [0.0, 1.0)
                ok!(Value::Float((next_rand_u64() >> 11) as f64 / 9_007_199_254_740_992.0));
            }
            "random_i64" => {
                want(2)?;
                let (lo, hi) = (as_int(&args[0])?, as_int(&args[1])?);
                if hi <= lo {
                    ok!(Value::Int(lo));
                }
                let range = (hi as i128 - lo as i128) as u128;
                ok!(Value::Int(lo + (next_rand_u64() as u128 % range) as i64));
            }
            "str_pad_start" => {
                want(3)?;
                let s = as_str(&args[0])?;
                let width = as_int(&args[1])?.max(0) as usize;
                let fill = as_str(&args[2])?.chars().next().unwrap_or(' ');
                ok!(Value::Str(if s.len() >= width {
                    s.to_string()
                } else {
                    format!("{}{}", fill.to_string().repeat(width - s.len()), s)
                }));
            }
            "str_pad_end" => {
                want(3)?;
                let s = as_str(&args[0])?;
                let width = as_int(&args[1])?.max(0) as usize;
                let fill = as_str(&args[2])?.chars().next().unwrap_or(' ');
                ok!(Value::Str(if s.len() >= width {
                    s.to_string()
                } else {
                    format!("{}{}", s, fill.to_string().repeat(width - s.len()))
                }));
            }
            "i64_to_str_radix" => {
                want(2)?;
                let n = as_int(&args[0])?;
                let base = as_int(&args[1])?;
                if !(2..=36).contains(&base) {
                    return panic(format!("i64_to_str_radix: radix must be 2..=36, got {base}"));
                }
                ok!(Value::Str(i64_to_radix(n, base as u32)));
            }
            "uncertain_new_f64" => {
                want(2)?;
                ok!(make_uncertain(Value::Float(as_float(&args[0])?), as_float(&args[1])?));
            }

            // ── Bit ops ───────────────────────────────────────────────────────
            "bit_and" => {
                want(2)?;
                ok!(Value::Int(as_int(&args[0])? & as_int(&args[1])?));
            }
            "bit_or" => {
                want(2)?;
                ok!(Value::Int(as_int(&args[0])? | as_int(&args[1])?));
            }
            "bit_xor" => {
                want(2)?;
                ok!(Value::Int(as_int(&args[0])? ^ as_int(&args[1])?));
            }
            "bit_not" => {
                want(1)?;
                ok!(Value::Int(!as_int(&args[0])?));
            }
            "shl" => {
                want(2)?;
                ok!(Value::Int(as_int(&args[0])?.wrapping_shl(as_int(&args[1])? as u32)));
            }
            "shr" => {
                want(2)?;
                ok!(Value::Int(as_int(&args[0])?.wrapping_shr(as_int(&args[1])? as u32)));
            }

            // ── String ops ──────────────────────────────────────────────────────
            "str_len" => {
                want(1)?;
                ok!(Value::Int(as_str(&args[0])?.len() as i64));
            }
            "str_concat" | "axon_concat" => {
                want(2)?;
                ok!(Value::Str(format!("{}{}", as_str(&args[0])?, as_str(&args[1])?)));
            }
            "str_eq" => {
                want(2)?;
                ok!(Value::Bool(as_str(&args[0])? == as_str(&args[1])?));
            }
            "str_contains" => {
                want(2)?;
                ok!(Value::Bool(as_str(&args[0])?.contains(as_str(&args[1])?)));
            }
            "str_starts_with" => {
                want(2)?;
                ok!(Value::Bool(as_str(&args[0])?.starts_with(as_str(&args[1])?)));
            }
            "str_ends_with" => {
                want(2)?;
                ok!(Value::Bool(as_str(&args[0])?.ends_with(as_str(&args[1])?)));
            }
            "str_to_upper" => {
                want(1)?;
                ok!(Value::Str(as_str(&args[0])?.to_uppercase()));
            }
            "str_to_lower" => {
                want(1)?;
                ok!(Value::Str(as_str(&args[0])?.to_lowercase()));
            }
            "str_trim" => {
                want(1)?;
                ok!(Value::Str(as_str(&args[0])?.trim().to_string()));
            }
            "str_trim_start" => {
                want(1)?;
                ok!(Value::Str(as_str(&args[0])?.trim_start().to_string()));
            }
            "str_trim_end" => {
                want(1)?;
                ok!(Value::Str(as_str(&args[0])?.trim_end().to_string()));
            }
            "str_reverse" => {
                want(1)?;
                ok!(Value::Str(as_str(&args[0])?.chars().rev().collect()));
            }
            "str_repeat" => {
                want(2)?;
                let n = as_int(&args[1])?.max(0) as usize;
                ok!(Value::Str(as_str(&args[0])?.repeat(n)));
            }
            "str_replace" => {
                want(3)?;
                ok!(Value::Str(as_str(&args[0])?.replace(as_str(&args[1])?, as_str(&args[2])?)));
            }
            "str_index_of" => {
                want(2)?;
                let hay = as_str(&args[0])?;
                let needle = as_str(&args[1])?;
                ok!(Value::Int(hay.find(needle).map(|i| i as i64).unwrap_or(-1)));
            }
            "str_count" => {
                want(2)?;
                ok!(Value::Int(as_str(&args[0])?.matches(as_str(&args[1])?).count() as i64));
            }
            "str_slice" => {
                want(3)?;
                let s = as_str(&args[0])?;
                let start = as_int(&args[1])?.max(0) as usize;
                let end = (as_int(&args[2])?.max(0) as usize).min(s.len());
                let start = start.min(end);
                ok!(Value::Str(s.get(start..end).unwrap_or("").to_string()));
            }
            "char_at" => {
                want(2)?;
                let s = as_str(&args[0])?;
                let i = as_int(&args[1])?.max(0) as usize;
                ok!(Value::Int(s.as_bytes().get(i).map(|b| *b as i64).unwrap_or(-1)));
            }

            // ── Assertions ──────────────────────────────────────────────────────
            "assert" => {
                want(1)?;
                if !as_bool(&args[0])? {
                    return Err(Flow::Panic("assertion failed".into()));
                }
                ok!(Value::Unit);
            }
            "assert_eq" => {
                want(2)?;
                let (a, b) = (as_int(&args[0])?, as_int(&args[1])?);
                if a != b {
                    return Err(Flow::Panic(format!("assertion failed: {a} != {b}")));
                }
                ok!(Value::Unit);
            }
            "assert_eq_str" => {
                want(2)?;
                let (a, b) = (as_str(&args[0])?, as_str(&args[1])?);
                if a != b {
                    return Err(Flow::Panic(format!("assertion failed: {a:?} != {b:?}")));
                }
                ok!(Value::Unit);
            }
            "assert_eq_f64" => {
                want(2)?;
                let (a, b) = (as_float(&args[0])?, as_float(&args[1])?);
                if (a - b).abs() > 1e-9 {
                    return Err(Flow::Panic(format!("assertion failed: {a} != {b}")));
                }
                ok!(Value::Unit);
            }
            "assert_err" => {
                want(1)?;
                if as_bool(&args[0])? {
                    return Err(Flow::Panic("assert_err: expected Err, got Ok".into()));
                }
                ok!(Value::Unit);
            }

            // ── Environment / time / process ──────────────────────────────────
            "env_var" => {
                want(1)?;
                ok!(match std::env::var(as_str(&args[0])?) {
                    Ok(v) => Value::Ok(Box::new(Value::Str(v))),
                    Err(_) => Value::Err(Box::new(Value::Str("not set".into()))),
                });
            }
            "now_ms" => {
                want(0)?;
                ok!(Value::Int(now_ms()));
            }
            "sleep_ms" => {
                want(1)?;
                let ms = as_int(&args[0])?.max(0) as u64;
                std::thread::sleep(std::time::Duration::from_millis(ms));
                ok!(Value::Unit);
            }
            "exit" => {
                want(1)?;
                Err(Flow::Exit(as_int(&args[0])? as i32))
            }

            // ── ASI: numeric conversions ────────────────────────────────────
            "f64_to_i64" => {
                want(1)?;
                ok!(Value::Int(as_float(&args[0])? as i64));
            }
            "i64_to_f64" => {
                want(1)?;
                ok!(Value::Float(as_int(&args[0])? as f64));
            }

            // ── ASI: Uncertain<T> construction ──────────────────────────────
            // Represented as a struct `Uncertain { value, confidence }`, so
            // `.value` / `.confidence` field access works directly.
            "uncertain_new" | "uncertain_dyn_i64" => {
                want(2)?;
                ok!(make_uncertain(Value::Int(as_int(&args[0])?), as_float(&args[1])?));
            }
            "uncertain_dyn_f64" => {
                want(2)?;
                ok!(make_uncertain(Value::Float(as_float(&args[0])?), as_float(&args[1])?));
            }
            "uncertain_deterministic" => {
                want(1)?;
                ok!(make_uncertain(Value::Int(as_int(&args[0])?), 1.0));
            }
            "uncertain_confidence" => {
                // Compile-time confidence hint; no runtime effect.
                ok!(Value::Unit);
            }

            // ── ASI: Temporal<T> ────────────────────────────────────────────
            // Represented as `Temporal { value, horizon_ms, decay, created_ms }`.
            "temporal_now" => {
                want(0)?;
                ok!(Value::Int(now_ms()));
            }
            "temporal_new" => {
                want(3)?;
                ok!(make_temporal(
                    Value::Int(as_int(&args[0])?),
                    as_int(&args[1])?,
                    as_float(&args[2])?,
                    now_ms(),
                ));
            }
            "temporal_at" => {
                want(2)?;
                match &args[0] {
                    Value::Struct { fields, .. } => {
                        let value = fields.get("value").cloned().unwrap_or(Value::Int(0));
                        let horizon = fields.get("horizon_ms").and_then(as_int_opt).unwrap_or(0);
                        let decay = fields.get("decay").and_then(as_float_opt).unwrap_or(0.0);
                        let created = fields.get("created_ms").and_then(as_int_opt).unwrap_or(0);
                        let offset = as_int(&args[1])?;
                        ok!(make_temporal(value, horizon, decay, created + offset));
                    }
                    _ => panic("temporal_at: expected a Temporal value"),
                }
            }
            "temporal_is_valid" => {
                want(1)?;
                match &args[0] {
                    Value::Struct { fields, .. } => {
                        let horizon = fields.get("horizon_ms").and_then(as_int_opt).unwrap_or(0);
                        let created = fields.get("created_ms").and_then(as_int_opt).unwrap_or(0);
                        ok!(Value::Bool(now_ms() <= created + horizon));
                    }
                    _ => panic("temporal_is_valid: expected a Temporal value"),
                }
            }

            // ── ASI: goal-directed optimization ─────────────────────────────
            // Live hill-climb over an `@[adaptive] fn(i64) -> i64`, else a
            // retrospective best-observed lookup (mirrors axon-rt's goal.rs).
            "goal_run" => {
                want(3)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                let max_evals = as_int(&args[2])?;
                ok!(Value::Float(self.run_goal(&name, target, max_evals)?));
            }

            // Random-search strategy. Baseline against the hill-climb
            // path; useful for multi-modal objectives where the
            // gradient gets stuck in a local optimum.
            "goal_run_random" => {
                want(5)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                let n_samples = as_int(&args[2])?;
                let lo = as_int(&args[3])?;
                let hi = as_int(&args[4])?;
                ok!(Value::Float(self.run_goal_random(&name, target, n_samples, lo, hi)?));
            }

            // Multi-start hill climb. Random restarts + local refinement —
            // the standard recipe for escaping local optima while keeping
            // gradient-style convergence once a basin is found.
            "goal_run_multistart" => {
                want(6)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                let n_starts = as_int(&args[2])?;
                let evals_per_start = as_int(&args[3])?;
                let lo = as_int(&args[4])?;
                let hi = as_int(&args[5])?;
                ok!(Value::Float(self.run_goal_multistart(
                    &name, target, n_starts, evals_per_start, lo, hi
                )?));
            }

            // Warm-start variant: seeds the optimizer at the best prior
            // probe from in-memory provenance, rather than starting at the
            // origin. Pair with a previous goal_run / goal_clear pattern
            // to do iterative refinement: each call resumes where the last
            // left off. Falls through to a fresh run when the fn has no
            // prior provenance entry, so it's safe to call cold.
            "goal_continue" => {
                want(3)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                let max_evals = as_int(&args[2])?;
                ok!(Value::Float(self.run_goal_warm(&name, target, max_evals)?));
            }

            // Read back the best-scoring leading-i64 input observed for an
            // `@[adaptive]` fn. Pairs with `goal_run` — the optimizer logs
            // (input, score) on every call, so a follow-up call can
            // introspect "what probe got us closest to target?". For
            // multi-arg fns this returns the FIRST dim; use
            // `goal_best_inputs` to read the full tuple. Returns 0 when
            // the fn was never called or has no i64 leading param.
            "goal_best_input" => {
                want(2)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                ok!(Value::Int(self.best_input(&name, target)));
            }

            // All i64 input dims that produced the best score, as a slice.
            // The multi-arg companion to `goal_best_input`: for an
            // `@[adaptive] fn(x: i64, y: i64) -> i64` it returns `[x*, y*]`.
            // Empty slice when the fn was never called.
            "goal_best_inputs" => {
                want(2)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                ok!(self.best_inputs(&name, target));
            }

            // f64-flavored counterpart: returns the f64-prefix input tuple
            // of the best-scoring entry. Pairs with the f64 hill climb for
            // continuous-domain `@[adaptive] fn(f64, …) -> f64` searches.
            "goal_best_inputs_f64" => {
                want(2)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                ok!(self.best_inputs_f64(&name, target));
            }

            // Read the best observed score WITHOUT running another
            // optimization. Equivalent to `goal_run(name, target, 0)`
            // but doesn't mutate provenance — purely a query against
            // what's already been recorded. The right primitive for
            // "how am I doing?" checkpoints between rounds.
            "goal_best_score" => {
                want(2)?;
                let name = as_str(&args[0])?.to_string();
                let target = as_float(&args[1])?;
                ok!(Value::Float(self.best_observed(&name, target, 0)));
            }

            // Number of provenance entries recorded for an @[adaptive] fn.
            // Equivalent to `len(goal_history(name))` but O(1) — avoids
            // materializing the trace just to count it. Useful for budget
            // gates: `if goal_count("score") > 1000 { stop }`.
            "goal_count" => {
                want(1)?;
                let name = as_str(&args[0])?.to_string();
                let n = self.provenance.borrow().get(&name).map(|v| v.len()).unwrap_or(0);
                ok!(Value::Int(n as i64));
            }

            // Full optimization trace as a slice of `(input, score)` tuples,
            // in call order. The companion to goal_run / goal_best_input.
            "goal_history" => {
                want(1)?;
                let name = as_str(&args[0])?.to_string();
                ok!(self.history(&name));
            }

            // Reset the @[adaptive] provenance for `name` so the next
            // goal_run starts fresh. Returns the count of evicted records
            // (so a caller can sanity-check there was anything to clear).
            "goal_clear" => {
                want(1)?;
                let name = as_str(&args[0])?.to_string();
                ok!(Value::Int(self.clear(&name)));
            }

            // ── Dict (string-keyed map) ──────────────────────────────────────
            // Reference-shared like Chan: mutating builtins update the same
            // underlying state every handle sees. The workhorse for caches,
            // frequency tables, named state.
            "dict_new" => {
                want(0)?;
                ok!(Value::Dict(Rc::new(RefCell::new(std::collections::BTreeMap::new()))));
            }
            "dict_get" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_get: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let k = as_str(&args[1])?.to_string();
                ok!(match d.borrow().get(&k) {
                    Some(v) => Value::Some(Box::new(v.clone())),
                    None => Value::None,
                });
            }
            "dict_set" => {
                want(3)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_set: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let k = as_str(&args[1])?.to_string();
                d.borrow_mut().insert(k, args[2].clone());
                ok!(Value::Unit);
            }
            "dict_has" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_has: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let k = as_str(&args[1])?.to_string();
                ok!(Value::Bool(d.borrow().contains_key(&k)));
            }
            // `dict_remove(d, k)` — remove entry, return the prior value as
            // `Option<T>`. Mirrors the Map API of every reasonable language.
            "dict_remove" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_remove: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let k = as_str(&args[1])?.to_string();
                ok!(match d.borrow_mut().remove(&k) {
                    Some(v) => Value::Some(Box::new(v)),
                    None => Value::None,
                });
            }
            "dict_len" => {
                want(1)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_len: expected dict, got {}",
                        other.type_name()
                    )),
                };
                ok!(Value::Int(d.borrow().len() as i64));
            }
            // `dict_keys(d) -> [str]` — sorted by BTreeMap ordering, so
            // iteration is deterministic.
            "dict_keys" => {
                want(1)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_keys: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let keys: Vec<Value> = d.borrow().keys().map(|k| Value::Str(k.clone())).collect();
                ok!(Value::Array(keys));
            }
            // `dict_map_values(d, f) -> Dict` — transform every value via
            // a closure, keys preserved. Returns a FRESH dict; the input
            // is not mutated. The dict analogue of arr_map.
            "dict_map_values" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_map_values: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let f = args[1].clone();
                let mut out = std::collections::BTreeMap::new();
                let pairs: Vec<(String, Value)> = d
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (k, v) in pairs {
                    let nv = self.call_closure(f.clone(), vec![v])?;
                    out.insert(k, nv);
                }
                ok!(Value::Dict(Rc::new(RefCell::new(out))));
            }
            // `arr_enumerate(xs)` — turn `[a, b, c]` into `[(0, a), (1, b),
            // (2, c)]`. The "iterate with index" pattern's primitive
            // form. Pairs with `arr_map` so closures can see both index
            // and value without a `let i = 0` + manual increment.
            "arr_enumerate" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "arr_enumerate: expected array, got {}",
                        other.type_name()
                    )),
                };
                let mut out = Vec::with_capacity(xs.len());
                for (i, v) in xs.iter().enumerate() {
                    out.push(Value::Tuple(vec![Value::Int(i as i64), v.clone()]));
                }
                ok!(Value::Array(out));
            }
            // `arr_partition(xs, pred)` — split into `(yes, no)` tuple
            // where `yes` is the elements satisfying `pred` and `no` is
            // the rest. One pass over the input; the natural complement
            // to `arr_filter` (which only keeps the yes side).
            "arr_partition" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_partition: expected array, got {}",
                        other.type_name()
                    )),
                };
                let pred = args[1].clone();
                let mut yes = Vec::new();
                let mut no = Vec::new();
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x.clone()])?;
                    match r {
                        Value::Bool(true) => yes.push(x),
                        Value::Bool(false) => no.push(x),
                        other => return panic(format!(
                            "arr_partition: predicate must return bool, got {}",
                            other.type_name()
                        )),
                    }
                }
                ok!(Value::Tuple(vec![Value::Array(yes), Value::Array(no)]));
            }
            // `dict_get_or(d, k, default)` — get the value at `k`, or
            // return `default` if absent. Compresses the ubiquitous
            // `match dict_get(d, k) { Some(v) => v  None => default }`
            // pattern into one call. The `default` can be any Value.
            "dict_get_or" => {
                want(3)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_get_or: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let k = as_str(&args[1])?.to_string();
                let default = args[2].clone();
                let v = d.borrow().get(&k).cloned().unwrap_or(default);
                ok!(v);
            }
            // `dict_inc(d, k)` — atomically bump an i64 counter at `k`.
            // Initializes to 1 if absent. Returns the new value. The
            // standard "increment-a-counter" idiom — replaces the
            // four-line get-default-add-set dance with one call.
            "dict_inc" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_inc: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let k = as_str(&args[1])?.to_string();
                let mut m = d.borrow_mut();
                let cur = m.get(&k).cloned().unwrap_or(Value::Int(0));
                let n = match cur {
                    Value::Int(n) => n + 1,
                    other => return panic(format!(
                        "dict_inc: existing value at '{k}' is {}, not i64",
                        other.type_name()
                    )),
                };
                m.insert(k, Value::Int(n));
                ok!(Value::Int(n));
            }
            // `dict_filter(d, pred)` — keep entries where the predicate
            // `fn(str, V) -> bool` holds. Returns a fresh dict. The dict
            // analogue of arr_filter, with the closure seeing (key, value).
            "dict_filter" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_filter: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let pred = args[1].clone();
                let pairs: Vec<(String, Value)> = d
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                let mut out: std::collections::BTreeMap<String, Value> =
                    std::collections::BTreeMap::new();
                for (k, v) in pairs {
                    let keep = self.call_closure(
                        pred.clone(),
                        vec![Value::Str(k.clone()), v.clone()],
                    )?;
                    match keep {
                        Value::Bool(true) => { out.insert(k, v); }
                        Value::Bool(false) => {}
                        other => return panic(format!(
                            "dict_filter: predicate must return bool, got {}",
                            other.type_name()
                        )),
                    }
                }
                ok!(Value::Dict(Rc::new(RefCell::new(out))));
            }
            // `dict_to_pairs(d) -> [(str, V)]` — entries as a slice of
            // tuples in BTreeMap key order. The natural primitive for
            // "sort a dict by value": dict_to_pairs → arr_sort_by →
            // arr_take. Empty dict → empty slice.
            "dict_to_pairs" => {
                want(1)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_to_pairs: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let pairs: Vec<Value> = d
                    .borrow()
                    .iter()
                    .map(|(k, v)| Value::Tuple(vec![Value::Str(k.clone()), v.clone()]))
                    .collect();
                ok!(Value::Array(pairs));
            }
            // `dict_from_pairs(xs) -> Dict` — inverse: build a Dict from
            // a slice of `(str, V)` tuples. Duplicate keys: the LAST
            // pair wins (matches the conventional construction order).
            "dict_from_pairs" => {
                want(1)?;
                let xs = match &args[0] {
                    Value::Array(v) => v,
                    other => return panic(format!(
                        "dict_from_pairs: expected array of (str, V) tuples, got {}",
                        other.type_name()
                    )),
                };
                let mut out: std::collections::BTreeMap<String, Value> =
                    std::collections::BTreeMap::new();
                for v in xs {
                    let pair = match v {
                        Value::Tuple(t) if t.len() == 2 => t,
                        other => return panic(format!(
                            "dict_from_pairs: each element must be a 2-tuple (str, V), got {}",
                            other.type_name()
                        )),
                    };
                    let k = match &pair[0] {
                        Value::Str(s) => s.clone(),
                        other => return panic(format!(
                            "dict_from_pairs: tuple's first element must be str, got {}",
                            other.type_name()
                        )),
                    };
                    out.insert(k, pair[1].clone());
                }
                ok!(Value::Dict(Rc::new(RefCell::new(out))));
            }

            // `dict_to_str(d) -> str` — serialize a `Dict<str, str>` to a
            // stable line-oriented text format: `key1=value1\nkey2=value2…`.
            // Each entry on its own line; BTreeMap key order so the output
            // is deterministic (round-trippable + diff-friendly). Non-string
            // values are converted via `display`; keys containing `=` or
            // `\n` are rejected to keep the format unambiguous.
            "dict_to_str" => {
                want(1)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_to_str: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let mut out = String::new();
                for (k, v) in d.borrow().iter() {
                    if k.contains('=') || k.contains('\n') {
                        return panic(format!(
                            "dict_to_str: key '{k}' contains an unrepresentable char (= or newline)"
                        ));
                    }
                    let vs = display(v);
                    if vs.contains('\n') {
                        return panic(format!(
                            "dict_to_str: value for key '{k}' contains a newline (unsupported)"
                        ));
                    }
                    out.push_str(k);
                    out.push('=');
                    out.push_str(&vs);
                    out.push('\n');
                }
                ok!(Value::Str(out));
            }
            // `dict_from_str(s) -> Dict` — inverse of `dict_to_str`.
            // Splits `s` into lines, each line at the FIRST `=` into
            // (key, value). All values are stored as `str` — caller
            // converts via `parse_int` / `parse_float` if needed.
            // Trailing newline is allowed; empty lines are skipped.
            // Malformed lines (no `=`) panic — we'd rather fail loud
            // than silently corrupt state.
            "dict_from_str" => {
                want(1)?;
                let s = as_str(&args[0])?;
                let mut out = std::collections::BTreeMap::new();
                for line in s.lines() {
                    if line.is_empty() {
                        continue;
                    }
                    match line.split_once('=') {
                        Some((k, v)) => {
                            out.insert(k.to_string(), Value::Str(v.to_string()));
                        }
                        None => {
                            return panic(format!(
                                "dict_from_str: malformed line '{line}' (expected key=value)"
                            ));
                        }
                    }
                }
                ok!(Value::Dict(Rc::new(RefCell::new(out))));
            }

            // `dict_merge(d1, d2) -> Dict` — union of two dicts. Right-
            // biased on collision (values in `d2` win when both have the
            // same key). Returns a fresh dict; the inputs are unchanged.
            "dict_merge" => {
                want(2)?;
                let d1 = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_merge: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let d2 = match &args[1] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_merge: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let mut out: std::collections::BTreeMap<String, Value> =
                    d1.borrow().clone();
                for (k, v) in d2.borrow().iter() {
                    out.insert(k.clone(), v.clone());
                }
                ok!(Value::Dict(Rc::new(RefCell::new(out))));
            }

            // `arr_max_by(xs, key_fn)` / `arr_min_by(xs, key_fn)` —
            // pick the element that maximizes / minimizes the closure's
            // numeric output. Folds three calls (arr_map + arr_argmax +
            // index) into one. Panics on an empty array (no sensible
            // default for an unbounded ordering).
            "arr_max_by" | "arr_min_by" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "{name}: expected array, got {}",
                        other.type_name()
                    )),
                };
                if xs.is_empty() {
                    return panic(format!("{name}: array is empty"));
                }
                let key_fn = args[1].clone();
                let pick_max = name == "arr_max_by";
                let to_f = |v: Value| -> Result<f64, Flow> {
                    match v {
                        Value::Int(n) => Ok(n as f64),
                        Value::Float(f) => Ok(f),
                        other => Err(Flow::Panic(format!(
                            "{name}: key fn must return numeric, got {}",
                            other.type_name()
                        ))),
                    }
                };
                let mut best_idx = 0;
                let mut best_key = to_f(
                    self.call_closure(key_fn.clone(), vec![xs[0].clone()])?,
                )?;
                for (i, x) in xs.iter().enumerate().skip(1) {
                    let k = to_f(
                        self.call_closure(key_fn.clone(), vec![x.clone()])?,
                    )?;
                    if (pick_max && k > best_key) || (!pick_max && k < best_key) {
                        best_key = k;
                        best_idx = i;
                    }
                }
                ok!(xs[best_idx].clone());
            }
            // `arr_take_while(xs, pred)` — prefix that satisfies pred,
            // up to (not including) the first failing element. The
            // streaming-prefix dual of arr_filter.
            "arr_take_while" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_take_while: expected array, got {}",
                        other.type_name()
                    )),
                };
                let pred = args[1].clone();
                let mut out = Vec::new();
                for x in xs {
                    let r = self.call_closure(pred.clone(), vec![x.clone()])?;
                    match r {
                        Value::Bool(true) => out.push(x),
                        Value::Bool(false) => break,
                        other => return panic(format!(
                            "arr_take_while: predicate must return bool, got {}",
                            other.type_name()
                        )),
                    }
                }
                ok!(Value::Array(out));
            }
            // `arr_drop_while(xs, pred)` — skip leading elements that
            // satisfy pred, keep the rest. Complement of arr_take_while.
            "arr_drop_while" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_drop_while: expected array, got {}",
                        other.type_name()
                    )),
                };
                let pred = args[1].clone();
                let mut still_dropping = true;
                let mut out = Vec::new();
                for x in xs {
                    if still_dropping {
                        let r = self.call_closure(pred.clone(), vec![x.clone()])?;
                        match r {
                            Value::Bool(true) => continue,
                            Value::Bool(false) => still_dropping = false,
                            other => return panic(format!(
                                "arr_drop_while: predicate must return bool, got {}",
                                other.type_name()
                            )),
                        }
                    }
                    out.push(x);
                }
                ok!(Value::Array(out));
            }
            // `dict_each(d, f)` — iterate (k, v) pairs via a closure
            // for side effects. Closure takes (str, V); return is
            // ignored. Useful for "print every entry" or "write each
            // to disk" patterns where you don't want to materialize
            // a new dict via dict_map_values.
            "dict_each" => {
                want(2)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_each: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let f = args[1].clone();
                let pairs: Vec<(String, Value)> = d
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (k, v) in pairs {
                    let _ = self.call_closure(
                        f.clone(),
                        vec![Value::Str(k), v],
                    )?;
                }
                ok!(Value::Unit);
            }

            // `arr_group_by(xs, key_fn) -> Dict[str, [T]]` — bucket the
            // array by a closure that maps each element to a string key.
            // Stable: elements appear in their input order within each
            // bucket. The natural "frequency table" / "by-category"
            // reduction.
            "arr_group_by" => {
                want(2)?;
                let xs = match &args[0] {
                    Value::Array(v) => v.clone(),
                    other => return panic(format!(
                        "arr_group_by: expected array, got {}",
                        other.type_name()
                    )),
                };
                let key_fn = args[1].clone();
                let mut out: std::collections::BTreeMap<String, Vec<Value>> =
                    std::collections::BTreeMap::new();
                for x in xs {
                    let k = self.call_closure(key_fn.clone(), vec![x.clone()])?;
                    let key = match k {
                        Value::Str(s) => s,
                        other => return panic(format!(
                            "arr_group_by: key fn must return str, got {}",
                            other.type_name()
                        )),
                    };
                    out.entry(key).or_default().push(x);
                }
                let map = out
                    .into_iter()
                    .map(|(k, v)| (k, Value::Array(v)))
                    .collect();
                ok!(Value::Dict(Rc::new(RefCell::new(map))));
            }
            // `dict_values(d) -> [V]` — values in key-sorted order.
            "dict_values" => {
                want(1)?;
                let d = match &args[0] {
                    Value::Dict(d) => d.clone(),
                    other => return panic(format!(
                        "dict_values: expected dict, got {}",
                        other.type_name()
                    )),
                };
                let vals: Vec<Value> = d.borrow().values().cloned().collect();
                ok!(Value::Array(vals));
            }

            // ── ASI: live LLM calls (require `--features asi-runtime`) ───────
            "ai_complete" => {
                want(1)?;
                if ai_mock_enabled() {
                    ok!(Value::Ok(Box::new(Value::Str(
                        "Mock summary: the single most important fact, stated concisely.".to_string()
                    ))));
                }
                #[cfg(feature = "asi-runtime")]
                {
                    ok!(match axon_ai::complete(as_str(&args[0])?) {
                        Ok(s) => Value::Ok(Box::new(Value::Str(s))),
                        Err(e) => Value::Err(Box::new(Value::Str(e))),
                    });
                }
                #[cfg(not(feature = "asi-runtime"))]
                return panic(
                    "ai_complete requires --features asi-runtime (or set AXON_AI_MOCK=1 for a deterministic stub)",
                );
            }
            "ai_extract_uncertain_i64" => {
                want(1)?;
                if ai_mock_enabled() {
                    ok!(Value::Ok(Box::new(make_uncertain(Value::Int(1), 0.9))));
                }
                #[cfg(feature = "asi-runtime")]
                {
                    ok!(match axon_ai::complete_typed_uncertain_i64(as_str(&args[0])?) {
                        Ok((v, c)) => Value::Ok(Box::new(make_uncertain(Value::Int(v), c))),
                        Err(e) => Value::Err(Box::new(Value::Str(e))),
                    });
                }
                #[cfg(not(feature = "asi-runtime"))]
                return panic(
                    "ai_extract_uncertain_i64 requires --features asi-runtime (or set AXON_AI_MOCK=1)",
                );
            }
            "ai_extract_uncertain_f64" => {
                want(1)?;
                if ai_mock_enabled() {
                    ok!(Value::Ok(Box::new(make_uncertain(Value::Float(1.0), 0.9))));
                }
                #[cfg(feature = "asi-runtime")]
                {
                    ok!(match axon_ai::complete_typed_uncertain_f64(as_str(&args[0])?) {
                        Ok((v, c)) => Value::Ok(Box::new(make_uncertain(Value::Float(v), c))),
                        Err(e) => Value::Err(Box::new(Value::Str(e))),
                    });
                }
                #[cfg(not(feature = "asi-runtime"))]
                return panic(
                    "ai_extract_uncertain_f64 requires --features asi-runtime (or set AXON_AI_MOCK=1)",
                );
            }

            _ => Ok(None),
        }
    }
}

enum LoopStep {
    Break,
    Continue,
}

// ── Free helpers ──────────────────────────────────────────────────────────────

fn lit_to_val(lit: &Literal) -> Value {
    match lit {
        Literal::Int(n) => Value::Int(*n),
        Literal::Float(f) => Value::Float(*f),
        Literal::Bool(b) => Value::Bool(*b),
        Literal::Str(s) => Value::Str(s.clone()),
    }
}

fn type_name_of(ty: &crate::ast::AxonType) -> String {
    use crate::ast::AxonType::*;
    match ty {
        Named(n) => n.clone(),
        Generic { base, .. } => base.clone(),
        Ref(inner) => type_name_of(inner),
        DynTrait(n) => n.clone(),
        TypeParam(n) => n.clone(),
        Slice(_) => "[]".into(),
        Option(_) => "Option".into(),
        Result { .. } => "Result".into(),
        Chan(_) => "Chan".into(),
        Fn { .. } => "fn".into(),
        Tuple(_) => "tuple".into(),
        Union(_) => "union".into(),
    }
}

/// One step of a flattened place expression (for nested place assignment).
enum PlaceStep {
    Field(String),
    Index(usize),
}

fn as_int(v: &Value) -> Result<i64, Flow> {
    match v {
        Value::Int(n) => Ok(*n),
        other => panic(format!("expected i64, got {}", other.type_name())),
    }
}
fn as_float(v: &Value) -> Result<f64, Flow> {
    match v {
        Value::Float(f) => Ok(*f),
        other => panic(format!("expected f64, got {}", other.type_name())),
    }
}
fn as_bool(v: &Value) -> Result<bool, Flow> {
    match v {
        Value::Bool(b) => Ok(*b),
        other => panic(format!("expected bool, got {}", other.type_name())),
    }
}
fn as_str(v: &Value) -> Result<&str, Flow> {
    match v {
        Value::Str(s) => Ok(s),
        other => panic(format!("expected str, got {}", other.type_name())),
    }
}
fn as_int_opt(v: &Value) -> Option<i64> {
    match v {
        Value::Int(n) => Some(*n),
        _ => None,
    }
}
fn as_float_opt(v: &Value) -> Option<f64> {
    match v {
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

/// Whether deterministic mock-LLM responses are enabled (`AXON_AI_MOCK` set and
/// not "0"/empty). Lets the ASI demos run end-to-end with no API key, no
/// network, and no `asi-runtime` feature — for showcases, CI, and tests.
fn ai_mock_enabled() -> bool {
    std::env::var("AXON_AI_MOCK").map(|v| !v.is_empty() && v != "0").unwrap_or(false)
}

/// Milliseconds since the Unix epoch.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// The provenance log path: `$XDG_CACHE_HOME/axon/provenance.jsonl` (or
/// `$HOME/.cache/axon/...`), matching axon-rt's location.
fn provenance_log_path() -> Option<std::path::PathBuf> {
    let base = std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| std::path::PathBuf::from(h).join(".cache")))?;
    Some(base.join("axon").join("provenance.jsonl"))
}

/// Append one `@[adaptive]` return to the provenance JSONL, in the same shape
/// axon-rt writes (`ts_ms`/`fn`/`event`/`payload`/`score`), so `axon trace`
/// and the observability tools see interpreter runs. Best-effort (errors
/// ignored — provenance is advisory, not load-bearing).
fn append_provenance_jsonl(fn_name: &str, payload: &str, score: f64, input: Option<i64>) {
    let Some(path) = provenance_log_path() else { return };
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let ts = now_ms().max(0) as u64;
    let s = if score.is_finite() { format!("{score}") } else { "0".to_string() };
    // `input` (the goal-search arg) is an additive field; axon-rt readers ignore it.
    let inp = match input {
        Some(x) => format!(",\"input\":{x}"),
        None => String::new(),
    };
    let line = format!(
        "{{\"ts_ms\":{ts},\"fn\":{f},\"event\":\"event\",\"payload\":{p},\"score\":{s}{inp}}}\n",
        f = json_quote(fn_name),
        p = json_quote(payload),
    );
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Minimal JSON string quoting (for the provenance log).
fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Cross-run continuation is opt-in via `AXON_GOAL_CONTINUE` (so default runs —
/// and tests — stay deterministic, starting each hill-climb from 0).
fn goal_continue_enabled() -> bool {
    std::env::var("AXON_GOAL_CONTINUE").map(|v| !v.is_empty() && v != "0").unwrap_or(false)
}

/// Read the persisted provenance JSONL and return the recorded `input` whose
/// `score` is closest to `target` for `fn_name` — i.e. the best prior search
/// position to resume a hill-climb from. `None` if no usable record exists.
fn read_best_input(fn_name: &str, target: f64) -> Option<i64> {
    let path = provenance_log_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let needle = format!("\"fn\":{}", json_quote(fn_name));
    let mut best_input: Option<i64> = None;
    let mut best_dist = f64::INFINITY;
    for line in content.lines() {
        if !line.contains(&needle) {
            continue;
        }
        let (Some(input), Some(score)) =
            (extract_json_num(line, "\"input\":"), extract_json_num(line, "\"score\":"))
        else {
            continue;
        };
        let dist = (score - target).abs();
        if dist < best_dist {
            best_dist = dist;
            best_input = Some(input as i64);
        }
    }
    best_input
}

/// Read the provenance JSONL and return the maximum `score` among entries
/// written at or after `since_ts_ms` (epoch ms). `axon goal --iterate` records
/// a start timestamp and calls this after each run to detect convergence — when
/// the best score stops improving — so it can stop early. Scoping by timestamp
/// ignores unrelated prior entries that share the (accumulating) log file.
/// `None` if the log is absent or has no qualifying scored entry.
pub fn best_recorded_score(since_ts_ms: u64) -> Option<f64> {
    let path = provenance_log_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let mut best: Option<f64> = None;
    for line in content.lines() {
        let ts = extract_json_num(line, "\"ts_ms\":").unwrap_or(0.0);
        if (ts as u64) < since_ts_ms {
            continue;
        }
        if let Some(s) = extract_json_num(line, "\"score\":") {
            best = Some(best.map_or(s, |b| b.max(s)));
        }
    }
    best
}

/// One parsed provenance record — the fields `axon trace` reports on.
pub struct ProvRecord {
    pub ts_ms: u64,
    pub func: String,
    pub score: f64,
    pub input: Option<i64>,
}

/// Read and parse the provenance JSONL (best-effort; malformed lines skipped).
/// `path` defaults to the standard log location. Returns `None` only if the log
/// file is absent/unreadable. Used by `axon trace`.
pub fn read_provenance(path: Option<&std::path::Path>) -> Option<Vec<ProvRecord>> {
    let owned;
    let path = match path {
        Some(p) => p,
        None => {
            owned = provenance_log_path()?;
            &owned
        }
    };
    let content = std::fs::read_to_string(path).ok()?;
    let mut out = Vec::new();
    for line in content.lines() {
        let (Some(func), Some(score)) =
            (extract_json_str(line, "\"fn\":"), extract_json_num(line, "\"score\":"))
        else {
            continue;
        };
        out.push(ProvRecord {
            ts_ms: extract_json_num(line, "\"ts_ms\":").unwrap_or(0.0) as u64,
            func,
            score,
            input: extract_json_num(line, "\"input\":").map(|x| x as i64),
        });
    }
    Some(out)
}

/// Extract the string value following `key` (e.g. `"fn":`) in a JSON line.
/// Tolerant of our own fixed log format; assumes no escaped quotes in the value
/// (true for fn names). `None` if absent or unterminated.
fn extract_json_str(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let rest = line[start..].trim_start().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Extract the numeric value following `key` in a JSON line (up to the next
/// `,` or `}`). Tolerant of our own fixed log format; not a general parser.
fn extract_json_num(line: &str, key: &str) -> Option<f64> {
    let start = line.find(key)? + key.len();
    let rest = &line[start..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    rest[..end].trim().parse::<f64>().ok()
}

/// A pseudo-random `u64` from a process-global xorshift state (seeded from the
/// clock on first use). Single-threaded interpreter, so no CAS needed.
fn next_rand_u64() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static STATE: AtomicU64 = AtomicU64::new(0);
    let mut x = STATE.load(Ordering::Relaxed);
    if x == 0 {
        x = (now_ms() as u64) | 1;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    STATE.store(x, Ordering::Relaxed);
    x
}

/// Render `n` in `base` (2–36), '-'-prefixed when negative.
fn i64_to_radix(n: i64, base: u32) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "0".to_string();
    }
    let neg = n < 0;
    let mut v = (n as i128).unsigned_abs(); // u128 — handles i64::MIN
    let b = base as u128;
    let mut buf = Vec::new();
    while v > 0 {
        buf.push(DIGITS[(v % b) as usize]);
        v /= b;
    }
    if neg {
        buf.push(b'-');
    }
    buf.reverse();
    String::from_utf8(buf).unwrap()
}

/// A numeric value coerced to `f64` for scoring, if it is numeric.
fn numeric_score(v: &Value) -> Option<f64> {
    match v {
        Value::Int(n) => Some(*n as f64),
        Value::Float(f) => Some(*f),
        _ => None,
    }
}

/// Build an `Uncertain { value, confidence }` struct value.
fn make_uncertain(value: Value, confidence: f64) -> Value {
    let mut fields = HashMap::new();
    fields.insert("value".to_string(), value);
    fields.insert("confidence".to_string(), Value::Float(confidence));
    Value::Struct { name: "Uncertain".to_string(), fields }
}

/// Build a `Temporal { value, horizon_ms, decay, created_ms }` struct value.
fn make_temporal(value: Value, horizon_ms: i64, decay: f64, created_ms: i64) -> Value {
    let mut fields = HashMap::new();
    fields.insert("value".to_string(), value);
    fields.insert("horizon_ms".to_string(), Value::Int(horizon_ms));
    fields.insert("decay".to_string(), Value::Float(decay));
    fields.insert("created_ms".to_string(), Value::Int(created_ms));
    Value::Struct { name: "Temporal".to_string(), fields }
}

fn is_i64_type(ty: &crate::ast::AxonType) -> bool {
    matches!(ty, crate::ast::AxonType::Named(n) if n == "i64")
}

fn is_f64_type(ty: &crate::ast::AxonType) -> bool {
    matches!(ty, crate::ast::AxonType::Named(n) if n == "f64")
}

/// Apply a comparison `BinOp` to two floats (used by the `@[verify]` gate).
fn cmp_f64(op: &BinOp, a: f64, b: f64) -> bool {
    match op {
        BinOp::Gt => a > b,
        BinOp::GtEq => a >= b,
        BinOp::Lt => a < b,
        BinOp::LtEq => a <= b,
        BinOp::Eq => a == b,
        BinOp::NotEq => a != b,
        _ => false,
    }
}

fn eval_unary(op: &UnaryOp, v: Value) -> R {
    match (op, v) {
        (UnaryOp::Neg, Value::Int(n)) => Ok(Value::Int(-n)),
        (UnaryOp::Neg, Value::Float(f)) => Ok(Value::Float(-f)),
        (UnaryOp::Not, Value::Bool(b)) => Ok(Value::Bool(!b)),
        (UnaryOp::BitNot, Value::Int(n)) => Ok(Value::Int(!n)),
        // `&expr` is a no-op at runtime for a value interpreter.
        (UnaryOp::Ref, v) => Ok(v),
        (op, v) => panic(format!("cannot apply {op:?} to {}", v.type_name())),
    }
}

fn eval_binop_vals(op: &BinOp, l: Value, r: Value) -> R {
    use BinOp::*;
    use Value::{Bool, Float, Int, Str};
    match (op, l, r) {
        // Integer arithmetic — checked by default. Overflow is a *graceful
        // panic* (catchable, exits non-zero at the CLI), never a silent
        // wrap: a wrapped value masquerading as success is the worst class
        // of bug for an autonomous consumer (BUG_HUNT #6, ARCHITECTURE
        // INVARIANTS I-9). Use the `wrapping_*` builtins for intentional
        // modular arithmetic.
        (Add, Int(a), Int(b)) => a
            .checked_add(b)
            .map(Int)
            .ok_or_else(|| Flow::Panic(format!("integer overflow: {a} + {b} exceeds i64"))),
        (Sub, Int(a), Int(b)) => a
            .checked_sub(b)
            .map(Int)
            .ok_or_else(|| Flow::Panic(format!("integer overflow: {a} - {b} exceeds i64"))),
        (Mul, Int(a), Int(b)) => a
            .checked_mul(b)
            .map(Int)
            .ok_or_else(|| Flow::Panic(format!("integer overflow: {a} * {b} exceeds i64"))),
        (Div, Int(a), Int(b)) => {
            if b == 0 {
                return Err(Flow::Panic("integer division by zero".into()));
            }
            Ok(Int(a.wrapping_div(b)))
        }
        (Rem, Int(a), Int(b)) => {
            if b == 0 {
                return Err(Flow::Panic("integer remainder by zero".into()));
            }
            Ok(Int(a.wrapping_rem(b)))
        }
        // Float arithmetic
        (Add, Float(a), Float(b)) => Ok(Float(a + b)),
        (Sub, Float(a), Float(b)) => Ok(Float(a - b)),
        (Mul, Float(a), Float(b)) => Ok(Float(a * b)),
        (Div, Float(a), Float(b)) => Ok(Float(a / b)),
        (Rem, Float(a), Float(b)) => Ok(Float(a % b)),
        // String concat
        (Add, Str(a), Str(b)) => Ok(Str(a + &b)),
        // Integer comparisons
        (Eq, Int(a), Int(b)) => Ok(Bool(a == b)),
        (NotEq, Int(a), Int(b)) => Ok(Bool(a != b)),
        (Lt, Int(a), Int(b)) => Ok(Bool(a < b)),
        (Gt, Int(a), Int(b)) => Ok(Bool(a > b)),
        (LtEq, Int(a), Int(b)) => Ok(Bool(a <= b)),
        (GtEq, Int(a), Int(b)) => Ok(Bool(a >= b)),
        // Float comparisons
        (Eq, Float(a), Float(b)) => Ok(Bool(a == b)),
        (NotEq, Float(a), Float(b)) => Ok(Bool(a != b)),
        (Lt, Float(a), Float(b)) => Ok(Bool(a < b)),
        (Gt, Float(a), Float(b)) => Ok(Bool(a > b)),
        (LtEq, Float(a), Float(b)) => Ok(Bool(a <= b)),
        (GtEq, Float(a), Float(b)) => Ok(Bool(a >= b)),
        // Bool / string equality
        (Eq, Bool(a), Bool(b)) => Ok(Bool(a == b)),
        (NotEq, Bool(a), Bool(b)) => Ok(Bool(a != b)),
        (Eq, Str(a), Str(b)) => Ok(Bool(a == b)),
        (NotEq, Str(a), Str(b)) => Ok(Bool(a != b)),
        // Integer bitwise
        (BitAnd, Int(a), Int(b)) => Ok(Int(a & b)),
        (BitOr, Int(a), Int(b)) => Ok(Int(a | b)),
        (BitXor, Int(a), Int(b)) => Ok(Int(a ^ b)),
        (Shl, Int(a), Int(b)) => Ok(Int(a.wrapping_shl(b as u32))),
        (Shr, Int(a), Int(b)) => Ok(Int(a.wrapping_shr(b as u32))),
        // Structural equality for composite values (structs, enums, arrays,
        // Option/Result). Primitives are handled above; this catches the rest,
        // matching the `values_equal` used by `assert_eq`.
        (Eq, l, r) => Ok(Bool(values_equal(&l, &r))),
        (NotEq, l, r) => Ok(Bool(!values_equal(&l, &r))),
        (op, l, r) => panic(format!(
            "cannot apply {op:?} to {} / {}",
            l.type_name(),
            r.type_name()
        )),
    }
}

/// Structural equality for runtime values.
fn values_equal(a: &Value, b: &Value) -> bool {
    use Value::*;
    match (a, b) {
        (Int(x), Int(y)) => x == y,
        (Float(x), Float(y)) => x == y,
        (Bool(x), Bool(y)) => x == y,
        (Str(x), Str(y)) => x == y,
        (Unit, Unit) => true,
        (None, None) => true,
        (Some(x), Some(y)) | (Ok(x), Ok(y)) | (Err(x), Err(y)) => values_equal(x, y),
        (Array(x), Array(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| values_equal(p, q)),
        (Struct { name: n1, fields: f1 }, Struct { name: n2, fields: f2 }) => {
            n1 == n2 && fields_equal(f1, f2)
        }
        (
            Enum { enum_name: e1, variant: v1, fields: f1 },
            Enum { enum_name: e2, variant: v2, fields: f2 },
        ) => e1 == e2 && v1 == v2 && fields_equal(f1, f2),
        (Tuple(x), Tuple(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| values_equal(p, q))
        }
        (Dict(d1), Dict(d2)) => {
            // Two dicts are equal iff they have the same key set and the
            // values agree pairwise. Iterating BTreeMaps is sorted, so a
            // direct paired scan suffices.
            let m1 = d1.borrow();
            let m2 = d2.borrow();
            m1.len() == m2.len()
                && m1.iter().zip(m2.iter()).all(|((k1, v1), (k2, v2))| {
                    k1 == k2 && values_equal(v1, v2)
                })
        }
        _ => false,
    }
}

fn fields_equal(a: &HashMap<String, Value>, b: &HashMap<String, Value>) -> bool {
    a.len() == b.len()
        && a.iter().all(|(k, v)| b.get(k).map(|w| values_equal(v, w)).unwrap_or(false))
}

/// Render a value for `print`/`println`/string interpolation. A `str` renders
/// as its raw contents (no quotes); everything else gets a reasonable form.
fn display(v: &Value) -> String {
    match v {
        Value::Str(s) => s.clone(),
        Value::Int(n) => n.to_string(),
        Value::Float(f) => fmt_g(*f),
        Value::Bool(b) => b.to_string(),
        Value::Unit => "()".into(),
        Value::None => "None".into(),
        Value::Some(x) => format!("Some({})", display(x)),
        Value::Ok(x) => format!("Ok({})", display(x)),
        Value::Err(x) => format!("Err({})", display(x)),
        Value::Array(items) => {
            let inner: Vec<String> = items.iter().map(display).collect();
            format!("[{}]", inner.join(", "))
        }
        Value::Struct { name, fields } => format!("{name} {{ {} }}", fields_display(fields)),
        Value::Enum { enum_name, variant, fields } => {
            if fields.is_empty() {
                format!("{enum_name}::{variant}")
            } else {
                format!("{enum_name}::{variant} {{ {} }}", fields_display(fields))
            }
        }
        Value::Closure { .. } => "<fn>".into(),
        Value::Chan(q) => format!("<chan len={}>", q.borrow().len()),
        Value::Tuple(items) => {
            let parts: Vec<String> = items.iter().map(display).collect();
            format!("({})", parts.join(", "))
        }
        Value::Dict(d) => {
            let m = d.borrow();
            let parts: Vec<String> = m
                .iter()
                .map(|(k, v)| format!("{k}: {}", display(v)))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

fn fields_display(fields: &HashMap<String, Value>) -> String {
    let mut parts: Vec<String> = fields.iter().map(|(k, v)| format!("{k}: {}", display(v))).collect();
    parts.sort();
    parts.join(", ")
}

/// Approximate C's `%.6g` (used by codegen's `to_str_f64`): 6 significant
/// digits, trailing zeros trimmed.
fn fmt_g(x: f64) -> String {
    if x == 0.0 {
        return "0".into();
    }
    if x.is_nan() {
        return "nan".into();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf" } else { "inf" }.into();
    }
    let p: i32 = 6;
    let e = x.abs().log10().floor() as i32;
    if e < -4 || e >= p {
        // Exponential form; trim trailing zeros in the mantissa.
        let s = format!("{:.*e}", (p - 1) as usize, x);
        return s;
    }
    let decimals = (p - 1 - e).max(0) as usize;
    let mut s = format!("{:.*}", decimals, x);
    if s.contains('.') {
        while s.ends_with('0') {
            s.pop();
        }
        if s.ends_with('.') {
            s.pop();
        }
    }
    s
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn run(src: &str) -> i32 {
        let program = crate::parse_source(src).expect("parse failed");
        run_program(&program)
    }

    #[test]
    fn hello_runs_and_exits_zero() {
        assert_eq!(run(r#"fn main() { println("hi") }"#), 0);
    }

    #[test]
    fn main_returns_exit_code() {
        assert_eq!(run("fn main() -> i64 { 7 }"), 7);
    }

    #[test]
    fn arithmetic_and_while() {
        let src = r#"
            fn main() -> i64 {
                let acc = 0
                let i = 1
                while i <= 10 { acc = acc + i  i = i + 1 }
                acc
            }
        "#;
        assert_eq!(run(src), 55);
    }

    #[test]
    fn recursion() {
        let src = r#"
            fn fib(n: i64) -> i64 {
                if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
            }
            fn main() -> i64 { fib(10) }
        "#;
        assert_eq!(run(src), 55);
    }

    #[test]
    fn result_match_and_question() {
        let src = r#"
            fn half(n: i64) -> Result<i64, str> {
                if n % 2 == 0 { Ok(n / 2) } else { Err("odd") }
            }
            fn doit(n: i64) -> Result<i64, str> {
                let h = half(n)?
                Ok(h + 1)
            }
            fn main() -> i64 {
                match doit(10) { Ok(v) => v  Err(_) => -1 }
            }
        "#;
        assert_eq!(run(src), 6);
    }

    #[test]
    fn enum_match() {
        let src = r#"
            enum Shape { Circle { r: i64 }, Square { s: i64 } }
            fn area(x: Shape) -> i64 {
                match x { Shape::Circle { r } => r * r  Shape::Square { s } => s * s }
            }
            fn main() -> i64 { area(Shape::Square { s: 4 }) }
        "#;
        assert_eq!(run(src), 16);
    }

    #[test]
    fn closures_capture() {
        let src = r#"
            fn make_adder(n: i64) -> (i64) -> i64 { (x: i64) => x + n }
            fn main() -> i64 {
                let add10 = make_adder(10)
                add10(5)
            }
        "#;
        assert_eq!(run(src), 15);
    }

    #[test]
    fn for_loop_break_continue() {
        let src = r#"
            fn main() -> i64 {
                let total = 0
                for i in 0..10 {
                    if i == 5 { break }
                    total = total + i
                }
                total
            }
        "#;
        assert_eq!(run(src), 10); // 0+1+2+3+4
    }

    #[test]
    fn struct_field_access() {
        let src = r#"
            type Point = { x: i64, y: i64 }
            fn main() -> i64 {
                let p = Point { x: 3, y: 4 }
                p.x + p.y
            }
        "#;
        assert_eq!(run(src), 7);
    }

    // ── ASI (M2) ──────────────────────────────────────────────────────────

    #[test]
    fn uncertain_construction_and_field_access() {
        let src = r#"
            fn main() -> i64 {
                let u = uncertain_new(42, 0.75)
                u.value
            }
        "#;
        assert_eq!(run(src), 42);
    }

    #[test]
    fn numeric_conversions() {
        let src = r#"
            fn main() -> i64 { f64_to_i64(i64_to_f64(9) + 0.9) }
        "#;
        assert_eq!(run(src), 9); // 9.9 truncates toward zero
    }

    #[test]
    fn goal_run_hill_climbs_adaptive_fn() {
        // score(x) = 100 - (x-7)^2, peak 100 at x=7. goal_run should climb to it.
        let src = r#"
            @[adaptive(metric: s, target: 100)]
            fn score(x: i64) -> i64 {
                let d = x - 7
                100 - d * d
            }
            fn main() -> i64 {
                let best = goal_run("score", 100.0, 64)
                f64_to_i64(best)
            }
        "#;
        assert_eq!(run(src), 100);
    }

    #[test]
    fn goal_run_retrospective_picks_closest() {
        // measure returns f64 → not hill-climb-eligible → retrospective path
        // over logged records [20, 40, 60]; closest to 50 is 40 (earliest of tie).
        let src = r#"
            @[adaptive(metric: m, target: 50)]
            fn measure(x: i64) -> f64 { i64_to_f64(x) }
            fn main() -> i64 {
                let _ = measure(20)
                let _ = measure(40)
                let _ = measure(60)
                f64_to_i64(goal_run("measure", 50.0, 100))
            }
        "#;
        assert_eq!(run(src), 40);
    }

    #[test]
    fn goal_run_returns_target_when_no_records() {
        let src = r#"
            fn main() -> i64 { f64_to_i64(goal_run("never_called", 70.0, 20)) }
        "#;
        assert_eq!(run(src), 70);
    }

    #[test]
    fn verify_gate_passes_when_confident() {
        let src = r#"
            @[verify(confidence >= 0.8)]
            fn gate(c: f64) -> Uncertain<i64> { uncertain_dyn_i64(1, c) }
            fn main() -> i64 {
                let u = gate(0.9)
                u.value
            }
        "#;
        assert_eq!(run(src), 1);
    }

    #[test]
    fn verify_gate_panics_when_underconfident() {
        let src = r#"
            @[verify(confidence >= 0.8)]
            fn gate(c: f64) -> Uncertain<i64> { uncertain_dyn_i64(1, c) }
            fn main() -> i64 {
                let u = gate(0.5)
                u.value
            }
        "#;
        assert_eq!(run(src), 101); // verify gate fires → panic exit code
    }

    #[test]
    fn extended_math_builtins() {
        // clamp_i64 / sign_i64 / pow_i64 / min_f64 / max_f64 / clamp_f64
        let src = r#"
            fn main() -> i64 {
                let i = clamp_i64(150, 0, 100) + clamp_i64(-5, 0, 100) + sign_i64(-7) + pow_i64(2, 10)
                // 100 + 0 + (-1) + 1024 = 1123
                let f = f64_to_i64(min_f64(3.5, 2.5) + max_f64(1.0, 4.0) + clamp_f64(9.9, 0.0, 5.0))
                // 2.5 + 4.0 + 5.0 = 11.5 -> 11
                i + f
            }
        "#;
        assert_eq!(run(src), 1134);
    }

    #[test]
    fn more_builtins_coverage() {
        // sqrt_f64 / round_f64 / str_pad_start / str_pad_end / i64_to_str_radix / uncertain_new_f64
        let src = r#"
            fn main() -> i64 {
                let a = f64_to_i64(sqrt_f64(144.0))                  // 12
                let b = f64_to_i64(round_f64(2.6))                   // 3
                let p = str_len(str_pad_start("7", 4, "0"))          // "0007" -> 4
                let q = str_len(str_pad_end("7", 2, "x"))            // "7x" -> 2
                let r = if str_eq(i64_to_str_radix(255, 16), "ff") { 1 } else { 0 } // 1
                let u = f64_to_i64(uncertain_new_f64(5.0, 0.9).value) // 5
                a + b + p + q + r + u
            }
        "#;
        assert_eq!(run(src), 27);
    }

    #[test]
    fn ai_mock_mode_returns_ok() {
        // With AXON_AI_MOCK set, ai_complete returns Ok(non-empty) with no key /
        // network / asi-runtime feature. (No other test reads this env var.)
        std::env::set_var("AXON_AI_MOCK", "1");
        let n = run(r#"
            fn main() -> i64 {
                match ai_complete("anything") {
                    Ok(s) => str_len(s)
                    Err(_) => -1
                }
            }
        "#);
        std::env::remove_var("AXON_AI_MOCK");
        assert!(n > 0, "mock ai_complete should return Ok(non-empty), got {n}");
    }

    #[test]
    fn goal_demos_pure_outcomes() {
        // Regression lock for the key-free goal demos: prose → .ax → run,
        // pinning each gate path. (No LLM / no env — the mock-requiring goals
        // hello/flagship are exercised separately.)
        let base = concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples/goals/");
        for (file, expected) in [
            ("optimize-goal.md", 0),   // deploys
            ("compose-goal.md", 0),    // deploys (prelude-composed score)
            ("verified-goal.md", 101), // enforced confidence gate blocks
            ("redteam-goal.md", 1),    // redteam gate blocks
        ] {
            let md = std::fs::read_to_string(format!("{base}{file}"))
                .unwrap_or_else(|e| panic!("read {file}: {e}"));
            let goal = axon_surface::parser::GoalFile::parse(&md)
                .unwrap_or_else(|e| panic!("parse {file}: {e}"));
            let ax = axon_surface::compile::emit(&goal)
                .unwrap_or_else(|e| panic!("emit {file}: {e}"));
            let program =
                crate::parse_source(&ax).unwrap_or_else(|e| panic!("parse .ax for {file}: {e}"));
            let code = run_program(&program);
            assert_eq!(code, expected, "{file}: expected exit {expected}, got {code}");
        }
    }

    #[test]
    fn all_example_ax_files_parse() {
        // Broad regression guard: every .ax under examples/ must parse (covers
        // the basic examples, the asi "public face", stdlib, and modular libs).
        fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            if let Ok(entries) = std::fs::read_dir(dir) {
                for e in entries.flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        collect(&p, out);
                    } else if p.extension().map(|x| x == "ax").unwrap_or(false) {
                        out.push(p);
                    }
                }
            }
        }
        let root = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../examples"));
        let mut files = Vec::new();
        collect(root, &mut files);
        assert!(files.len() >= 20, "expected many example .ax files, found {}", files.len());
        for f in &files {
            let src = std::fs::read_to_string(f).unwrap_or_else(|e| panic!("read {}: {e}", f.display()));
            if let Err(e) = crate::parse_source(&src) {
                panic!("{} failed to parse: {e}", f.display());
            }
        }
    }
}
