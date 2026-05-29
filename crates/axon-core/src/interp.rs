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

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::Write as _;

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
}

/// Parse-and-run convenience: returns the process exit code.
pub fn run_program(program: &Program) -> i32 {
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
        if f.params.len() != args.len() {
            return panic(format!(
                "{}: expected {} args, got {}",
                f.name,
                f.params.len(),
                args.len()
            ));
        }
        // The leading i64 arg (if any) is the goal-search input — recorded with
        // the provenance so goal_run can resume from the best prior input.
        let input_arg = args.first().and_then(|v| match v {
            Value::Int(n) => Some(*n),
            _ => None,
        });
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
                // Also persist to the provenance JSONL (axon-rt's format) so
                // `axon trace`/observability tooling see interpreter runs.
                let payload = match &result {
                    Value::Int(n) => format!("ret_i64={n}"),
                    _ => format!("ret_f64={score}"),
                };
                append_provenance_jsonl(&f.name, &payload, score, input_arg);
            }
        }

        // `@[verify(confidence OP K)]`: runtime gate. Matches codegen, which only
        // emits `__axon_verify_panic` for the decodable confidence-predicate shape
        // on an `Uncertain<_>` return; other predicate shapes are no-ops at runtime.
        if let Some(spec) = &f.verify {
            if let Some((op, bound)) = crate::verify::decode_verify_predicate(&spec.predicate) {
                if let Value::Struct { name, fields } = &result {
                    if name == "Uncertain" {
                        if let Some(Value::Float(c)) = fields.get("confidence") {
                            if !cmp_f64(&op, *c, bound) {
                                return Err(Flow::Panic(format!(
                                    "verify failed in `{}`: confidence {} {} {} is false",
                                    f.name,
                                    c,
                                    crate::verify::binop_to_verify_str(&op),
                                    bound
                                )));
                            }
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
    fn run_goal(&self, name: &str, target: f64, max_evals: i64) -> Result<f64, Flow> {
        if let Some(f) = self.fns.get(name) {
            let f = *f;
            let eligible = f.attrs.iter().any(|a| a.name == "adaptive")
                && f.params.len() == 1
                && is_i64_type(&f.params[0].ty)
                && f.return_type.as_ref().map(is_i64_type).unwrap_or(false);
            if eligible {
                return self.hill_climb_i64(f, target, max_evals);
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
        let mut step: i64 = std::cmp::max(1, (cur_input.unsigned_abs() as i64) / 4);

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
            }

            if improved {
                cur_input = best_input;
            } else {
                step /= 2;
            }
        }
        Ok(best_score)
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

            Expr::Let { name, value }
            | Expr::Own { name, value }
            | Expr::RefBind { name, value } => {
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

            // Place assignment: `ident[i] = v` / `ident.field = v`. Single-level
            // (the receiver is a plain identifier); the value interpreter mutates
            // the binding in place. Nested places (`a.b[i]`, `a[i].f`) aren't
            // supported yet.
            Expr::AssignTo { place, value } => {
                let v = self.eval(value, env)?;
                match place.as_ref() {
                    Expr::Index { receiver, index } => {
                        let Expr::Ident(name) = receiver.as_ref() else {
                            return panic("only `ident[i] = v` place assignment is supported");
                        };
                        let idx = as_int(&self.eval(index, env)?)?;
                        let slot = env
                            .get_mut(name)
                            .ok_or_else(|| Flow::Panic(format!("assignment to undefined variable `{name}`")))?;
                        match slot {
                            Value::Array(items) => {
                                if idx < 0 || idx as usize >= items.len() {
                                    return panic(format!("index {idx} out of bounds (len {})", items.len()));
                                }
                                items[idx as usize] = v;
                                Ok(Value::Unit)
                            }
                            other => panic(format!("cannot index-assign into {}", other.type_name())),
                        }
                    }
                    Expr::FieldAccess { receiver, field } => {
                        let Expr::Ident(name) = receiver.as_ref() else {
                            return panic("only `ident.field = v` place assignment is supported");
                        };
                        let slot = env
                            .get_mut(name)
                            .ok_or_else(|| Flow::Panic(format!("assignment to undefined variable `{name}`")))?;
                        match slot {
                            Value::Struct { fields, .. } => {
                                fields.insert(field.clone(), v);
                                Ok(Value::Unit)
                            }
                            other => panic(format!("cannot field-assign into {}", other.type_name())),
                        }
                    }
                    _ => panic("invalid assignment target"),
                }
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

            Expr::Call { callee, args } => self.eval_call(callee, args, env),

            Expr::MethodCall { receiver, method, args } => {
                let recv = self.eval(receiver, env)?;
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
                    other => panic(format!("field access on non-struct ({})", other.type_name())),
                }
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

            Expr::Spawn(_) | Expr::Select(_) => {
                panic("channels/concurrency are not supported by the interpreter")
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
            Pattern::Tuple(_) => Ok(false), // tuple values are not constructible yet
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
                    ok!(Value::Str(String::new()));
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
        // Integer arithmetic
        (Add, Int(a), Int(b)) => Ok(Int(a.wrapping_add(b))),
        (Sub, Int(a), Int(b)) => Ok(Int(a.wrapping_sub(b))),
        (Mul, Int(a), Int(b)) => Ok(Int(a.wrapping_mul(b))),
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
